//! Step 2's map pictures: each card shows the board its match is played
//! on, drawn by the board renderer itself into a picture of its own.
//!
//! A card's board is built by the one function a local round is built by
//! ([`local_board`]), on the seed the match will be played on
//! ([`MatchMenu::seed`]), run a moment past its first tick so the crabs
//! are out, and laid out by the round's own spawners ([`lay_out_board`])
//! on a render layer of its own. A camera on that layer draws it into an
//! image until the renderer has drawn a few frames with every pipeline
//! compiled ([`PreviewPipelines`]), and then the camera and the board go.
//! Only the image stays, until match setup is left. The cards on show are
//! drawn first, a few a frame ([`PREVIEWS_PER_FRAME`]), and a card is
//! drawn again only when the board it shows changes. So the pictures cost
//! a handful of frames when step 2 opens or the seat count moves, and
//! nothing at all the rest of the time.

use super::*;
use crate::app::art::Art;
use crate::app::layout::{self, TILE};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::ecs::world::CommandQueue;
use bevy::render::render_resource::TextureFormat;

/// A preview picture's size, in pixels: twice the card's thumbnail, so it
/// stays sharp on a window the interface is scaled up in.
pub const PREVIEW_PX: UVec2 = UVec2::new(2 * ui::THUMB_W as u32, 2 * ui::THUMB_H as u32);

/// How many ticks past the start a card's board is shown at: about two
/// seconds of play, the crabs out of their holes and nobody's arrow down
/// yet.
pub const PREVIEW_TICKS: u32 = 40;

/// How many frames a preview's camera draws at least, and how many frames
/// in a row the renderer must have drawn with no pipeline still compiling
/// ([`PreviewPipelines`]), before the camera and its board go: a frame
/// drawn while a shader compiled on first use is not ready shows the clear
/// colour alone, and on a slow first compile a fixed few frames left a
/// card blank all visit.
pub const PREVIEW_FRAMES: u32 = 6;

/// How many frames a preview's camera waits for the renderer to settle
/// before it gives up: the picture is let go, and drawn again from the
/// start, so no card is left blank for good, and no camera draws forever.
pub const PREVIEW_PATIENCE: u32 = 600;

/// How many cards' previews start drawing in one frame: each lays a whole
/// board out, and twenty-odd at once was one long frame as step 2 opened.
pub const PREVIEWS_PER_FRAME: usize = 2;

/// The first render layer the previews use, one each above it. The game
/// draws on layer 0, which no preview touches.
const FIRST_LAYER: usize = 8;

/// A camera drawing a map card's picture, which every system that fits or
/// shakes the game's own camera leaves alone.
#[derive(Component)]
pub struct PreviewCamera;

/// The game's own camera, which the fit and the shake move: every
/// [`Camera2d`] but the previews'.
pub type GameCamera = (With<Camera2d>, Without<PreviewCamera>);

/// A sprite of a preview's board.
#[derive(Component)]
pub struct PreviewPart;

/// What a card's picture shows: everything [`local_board`] builds the
/// card's board from, on the config it is drawn with ([`card_config`]),
/// and whether it is greyed. Redrawn when any of it changes, and only
/// then: a greyed card is drawn at a table of its own, so the table's
/// count moving does not redraw it.
#[derive(Clone, PartialEq, Debug)]
struct Shows {
    seats: u8,
    map: MapChoice,
    custom: usize,
    gulls: GullPressure,
    round: RoundLength,
    posts: u8,
    seed: u64,
    greyed: bool,
}

impl Shows {
    fn of(played: &MatchConfig, seed: u64, greyed: bool) -> Shows {
        Shows {
            seats: played.seats,
            map: played.map,
            custom: played.custom,
            gulls: played.gulls,
            round: played.round,
            posts: played.posts,
            seed,
            greyed,
        }
    }
}

/// One card's picture, and while it is being drawn, what draws it.
struct Shot {
    shows: Shows,
    image: Handle<Image>,
    /// Whether the picture is known drawn: its camera stayed until the
    /// renderer had settled. A picture given up on is not, and is drawn
    /// again.
    drawn: bool,
    /// The camera and the board's sprites, and the frames they have drawn.
    drawing: Option<(Vec<Entity>, u32)>,
}

/// How many frames in a row the renderer has finished with no pipeline
/// waiting to compile: shared with the render world, which counts them
/// ([`watch_preview_pipelines`]). A preview's camera stays until this
/// reaches [`PREVIEW_FRAMES`]. Absent (a headless app has no renderer),
/// nothing is waited for.
#[derive(Resource, Clone, Default)]
pub struct PreviewPipelines(std::sync::Arc<std::sync::atomic::AtomicU32>);

impl PreviewPipelines {
    /// The frames in a row the renderer has drawn settled.
    pub fn settled(&self) -> u32 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Count one rendered frame, settled or not. The render world is the
    /// one writer, so a load and a store do not race.
    pub fn count(&self, settled: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        let streak = match settled {
            true => self.0.load(Relaxed).saturating_add(1),
            false => 0,
        };
        self.0.store(streak, Relaxed);
    }
}

/// Share a [`PreviewPipelines`] counter with the render world, which
/// counts each frame it finishes with no pipeline waiting to compile.
pub fn watch_preview_pipelines(app: &mut App) {
    let pipelines = PreviewPipelines::default();
    app.insert_resource(pipelines.clone());
    if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
        render.insert_resource(pipelines).add_systems(
            bevy::render::Render,
            count_settled_frames.in_set(bevy::render::RenderSystems::Cleanup),
        );
    }
}

/// The render world's half of [`PreviewPipelines`]: after a frame is
/// drawn, whether anything it wanted was still compiling.
fn count_settled_frames(
    cache: Res<bevy::render::render_resource::PipelineCache>,
    pipelines: Res<PreviewPipelines>,
) {
    pipelines.count(cache.waiting_pipelines().next().is_none());
}

/// What a picture is of: a card's board, or a quarter of the Surprise
/// card's mosaic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pic {
    Card(Card),
    /// A Surprise beach in the `n`th of [`SURPRISE_SAMPLES`].
    Sample(usize),
    /// The beach of the round about to be played, for the line-up before
    /// an online match ([`crate::app::lineup`]).
    Played,
}

/// The styles the Surprise card's picture shows, one a quarter.
pub const SURPRISE_SAMPLES: [crate::sim::BeachStyle; 4] = [
    crate::sim::BeachStyle::Maze,
    crate::sim::BeachStyle::Driftwood,
    crate::sim::BeachStyle::Islands,
    crate::sim::BeachStyle::Lagoon,
];

/// The table a Surprise sample is drawn at: four, on the first seed that
/// draws its style, so the picture is a real Surprise beach and the same
/// one every visit.
fn sample(config: &MatchConfig, quarter: usize) -> (MatchConfig, u64) {
    let style = SURPRISE_SAMPLES[quarter % SURPRISE_SAMPLES.len()];
    let seats = CLASSIC_SEATS;
    let seed = (1..)
        .find(|&seed| crate::sim::surprise_style(seed, seats) == style)
        .unwrap_or(1);
    let played = MatchConfig {
        seats,
        map: MapChoice::Surprise,
        random: false,
        ..config.clone()
    };
    (played, seed)
}

/// The pictures step 2's cards show, by what each is of.
#[derive(Resource, Default)]
pub struct MapPreviews {
    shots: Vec<(Pic, Shot)>,
}

impl MapPreviews {
    /// The picture of `pic`, once one has been drawn.
    pub fn image(&self, pic: Pic) -> Option<Handle<Image>> {
        self.shots
            .iter()
            .find(|(shot_pic, _)| *shot_pic == pic)
            .map(|(_, shot)| shot.image.clone())
    }

    /// How many cameras and board sprites are drawing a picture right now.
    pub fn drawing(&self) -> usize {
        self.shots
            .iter()
            .filter_map(|(_, shot)| shot.drawing.as_ref())
            .map(|(parts, _)| parts.len())
            .sum()
    }

    /// Whether `card`'s picture is being drawn right now.
    pub fn is_drawing(&self, card: Card) -> bool {
        self.shots
            .iter()
            .any(|(drawn, shot)| *drawn == Pic::Card(card) && shot.drawing.is_some())
    }

    /// How many pictures are known drawn.
    pub fn drawn(&self) -> usize {
        self.shots.iter().filter(|(_, shot)| shot.drawn).count()
    }

    /// How many pictures are held.
    pub fn held(&self) -> usize {
        self.shots.len()
    }
}

/// Draw the picture of each card on step 2 that has none, or whose
/// picture shows another board ([`Shows`]), a few a frame
/// ([`PREVIEWS_PER_FRAME`]): the cards on show first (and the maps the
/// Random card's mosaic borrows, when it is on show), the rest after.
/// Runs while step 2 is up; each picture is drawn once and then left
/// alone.
pub fn render_previews(world: &mut World) {
    // Step 2 on the couch, or the lobby host's map page over the lobby.
    let showing = world.get_resource::<MatchMenu>().is_some_and(|menu| {
        let screen = world
            .get_resource::<State<Screen>>()
            .map(|screen| *screen.get());
        menu.step == Step::Map
            && match menu.online {
                // The host's page, in the lobby or between rounds.
                true => matches!(screen, Some(Screen::Lobby | Screen::Versus)),
                false => screen == Some(Screen::MatchSetup),
            }
    });
    if !showing {
        return;
    }
    let config = world.resource::<MatchConfig>().clone();
    let menu = world.resource::<MatchMenu>();
    let (seed, scroll, online) = (menu.seed, menu.scroll, menu.online);
    let wanted: Vec<(usize, Pic, Shows, MatchConfig)> = {
        let beaches = world.resource::<CustomBeaches>();
        let previews = world.resource::<MapPreviews>();
        let slots = map_slots(beaches);
        let page = MapPage::of(&slots);
        // Each card with its place on the page, in card order: the card's
        // index is its picture's layer.
        let cards: Vec<(usize, Card)> = slots
            .iter()
            .enumerate()
            .filter_map(|(place, (_, slot))| match slot {
                Slot::Card(card) => Some((place, *card)),
                Slot::MakeOne => None,
            })
            .collect();
        let on_show = |place: usize| page.slot_in_view(scroll, place);
        let mosaic_on_show = |mosaic: Card| {
            cards
                .iter()
                .find(|(_, card)| *card == mosaic)
                .is_some_and(|(place, _)| on_show(*place))
        };
        let random_on_show = mosaic_on_show(Card::Random);
        let surprise_on_show = mosaic_on_show(Card::Map(MapChoice::Surprise));
        let current = |pic: Pic, shows: &Shows| {
            previews.shots.iter().any(|(drawn, shot)| {
                *drawn == pic && shot.shows == *shows && (shot.drawn || shot.drawing.is_some())
            })
        };
        // The cards' own pictures, each on the layer of its place.
        let mut wanted: Vec<(bool, usize, Pic, Shows, MatchConfig)> = cards
            .iter()
            .enumerate()
            .filter_map(|(index, &(place, card))| {
                let played = card_config(&config, card, beaches)?;
                let greyed = !card_open(card, config.seats, beaches, online);
                let shows = Shows::of(&played, seed, greyed);
                if current(Pic::Card(card), &shows) {
                    return None;
                }
                let first = on_show(place)
                    || (random_on_show && matches!(card, Card::Map(map) if MOSAIC.contains(&map)));
                Some((!first, index, Pic::Card(card), shows, played))
            })
            .collect();
        // Surprise's samples, on the layers past the last card's.
        for quarter in 0..SURPRISE_SAMPLES.len() {
            let (played, sample_seed) = sample(&config, quarter);
            let shows = Shows::of(&played, sample_seed, false);
            if !current(Pic::Sample(quarter), &shows) {
                wanted.push((
                    !surprise_on_show,
                    cards.len() + quarter,
                    Pic::Sample(quarter),
                    shows,
                    played,
                ));
            }
        }
        wanted.sort_by_key(|(later, index, ..)| (*later, *index));
        wanted
            .into_iter()
            .take(PREVIEWS_PER_FRAME)
            .map(|(_, index, pic, shows, played)| (index, pic, shows, played))
            .collect()
    };
    for (index, pic, shows, played) in wanted {
        let board = preview_board(&played, world.resource::<CustomBeaches>(), shows.seed);
        draw_card(world, index, pic, shows, &board);
    }
}

/// Draw `board` (the round's own, a moment in, as a card shows its beach)
/// into the picture of `pic`, by the same renderer and camera the cards'
/// pictures are drawn with. The picture is ready once
/// [`retire_previews`] has let its camera go, and goes with
/// [`release_previews`].
pub fn draw_board_picture(world: &mut World, pic: Pic, board: &crate::sim::Board) {
    let mut shown = board.clone();
    for _ in 0..PREVIEW_TICKS {
        shown.tick_idle();
    }
    let shows = Shows {
        seats: shown.seats_in_play(),
        map: MapChoice::Custom,
        custom: 0,
        gulls: GullPressure::Normal,
        round: RoundLength::Standard,
        posts: 0,
        seed: shown.seed(),
        greyed: false,
    };
    draw_card(world, 0, pic, shows, &shown);
}

/// The board a card shows: the one its match is played on, a moment in.
pub fn preview_board(
    config: &MatchConfig,
    beaches: &CustomBeaches,
    seed: u64,
) -> crate::sim::Board {
    let mut board = local_board(config, beaches, seed);
    for _ in 0..PREVIEW_TICKS {
        board.tick_idle();
    }
    board
}

/// Lay `board` out on its own layer and point a camera at it, drawing
/// into `card`'s picture (made now if the card has none).
fn draw_card(world: &mut World, index: usize, card: Pic, shows: Shows, board: &crate::sim::Board) {
    let layer = RenderLayers::layer(FIRST_LAYER + index);
    // Whatever was drawing this card's last picture goes first.
    let image = {
        let mut previews = world.resource_mut::<MapPreviews>();
        let old = previews
            .shots
            .iter_mut()
            .find(|(drawn, _)| *drawn == card)
            .and_then(|(_, shot)| shot.drawing.take());
        let image = previews
            .shots
            .iter()
            .find(|(drawn, _)| *drawn == card)
            .map(|(_, shot)| shot.image.clone());
        if let Some((parts, _)) = old {
            despawn_all(world, &parts);
        }
        image
    };
    let image = image.unwrap_or_else(|| {
        let mut target = Image::new_target_texture(
            PREVIEW_PX.x,
            PREVIEW_PX.y,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        );
        target.sampler = bevy::image::ImageSampler::linear();
        world.resource_mut::<Assets<Image>>().add(target)
    });
    let before: std::collections::HashSet<Entity> = world.query::<Entity>().iter(world).collect();
    world.resource_scope(|world, art: Mut<Art>| {
        let mut queue = CommandQueue::default();
        let mut commands = Commands::new(&mut queue, world);
        lay_out_board(&mut commands, board, &art);
        queue.apply(world);
    });
    let mut parts: Vec<Entity> = world
        .query::<Entity>()
        .iter(world)
        .filter(|entity| !before.contains(entity))
        .collect();
    // A still picture: what the round's systems find a sprite by comes
    // off, so none of them redraws or despawns it against its own board.
    for &part in &parts {
        world
            .entity_mut(part)
            .remove::<crate::app::board_render::LiveBoard>()
            .remove::<crate::app::creatures::LiveCreature>()
            .insert((PreviewPart, layer.clone()));
    }
    let camera = world
        .spawn((
            PreviewCamera,
            Camera2d,
            Camera {
                // Before the game's own camera, which shows the picture.
                order: -10 - index as isize,
                clear_color: ClearColorConfig::Custom(crate::app::materials::SAND),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Projection::Orthographic(OrthographicProjection {
                scale: preview_scale(board),
                ..OrthographicProjection::default_2d()
            }),
            bevy::camera::Hdr,
            bevy::core_pipeline::tonemapping::Tonemapping::Linear,
            preview_grade(shows.greyed),
            Msaa::Off,
            layer,
        ))
        .id();
    parts.push(camera);
    let shot = Shot {
        shows,
        image,
        drawn: false,
        drawing: Some((parts, 0)),
    };
    let mut previews = world.resource_mut::<MapPreviews>();
    match previews.shots.iter_mut().find(|(drawn, _)| *drawn == card) {
        Some((_, held)) => *held = shot,
        None => previews.shots.push((card, shot)),
    }
}

/// The board's sprites as a round lays them out, by the round's own
/// spawners, around the world's origin: the grid with its rocks, kelp,
/// holes and pools ([`crate::app::board_render::spawn_board_grid`]), what
/// changes in play at its state now, the castles, logs and posts
/// ([`crate::app::board_render::spawn_board_pieces`]), and the crabs and
/// gulls ([`crate::app::creatures::spawn_creatures`]).
fn lay_out_board(commands: &mut Commands, board: &crate::sim::Board, art: &Art) {
    crate::app::board_render::spawn_board_grid(commands, board, art);
    crate::app::board_render::spawn_board_pieces(commands, board, art);
    crate::app::creatures::spawn_creatures(commands, board, art);
}

/// World units per picture pixel that fit the board and its fence into
/// the picture, with a little sand round it.
fn preview_scale(board: &crate::sim::Board) -> f32 {
    let framed = layout::board_extent(board) + Vec2::splat(TILE * 0.7);
    (framed / PREVIEW_PX.as_vec2()).max_element()
}

/// The game's colour grade, and for a card the table cannot play, the
/// colour taken out and the light turned down: greyed, as the card says.
fn preview_grade(greyed: bool) -> bevy::render::view::ColorGrading {
    let mut grade = crate::app::boot::grade();
    if greyed {
        grade.global.post_saturation = 0.0;
        grade.global.exposure = -1.4;
    }
    grade
}

/// Count the frames each preview's camera has drawn, and once a picture is
/// drawn, take its camera and board away: after [`PREVIEW_FRAMES`] of its
/// own, with the renderer settled as long ([`PreviewPipelines`]). One that
/// waited [`PREVIEW_PATIENCE`] frames is given up on and drawn again.
pub fn retire_previews(world: &mut World) {
    let settled = world
        .get_resource::<PreviewPipelines>()
        .map_or(u32::MAX, PreviewPipelines::settled);
    let done: Vec<Entity> = {
        let mut previews = world.resource_mut::<MapPreviews>();
        let mut done = Vec::new();
        for (_, shot) in &mut previews.shots {
            let Some((parts, frames)) = &mut shot.drawing else {
                continue;
            };
            *frames += 1;
            let drawn = *frames >= PREVIEW_FRAMES && settled >= PREVIEW_FRAMES;
            if drawn || *frames >= PREVIEW_PATIENCE {
                done.append(parts);
                shot.drawing = None;
                shot.drawn = drawn;
            }
        }
        done
    };
    despawn_all(world, &done);
}

/// Leaving match setup: every camera and board still drawing goes, and
/// every picture is let go of, so nothing of the previews outlives the
/// screen.
pub fn release_previews(world: &mut World) {
    let shots = std::mem::take(&mut world.resource_mut::<MapPreviews>().shots);
    for (_, shot) in shots {
        if let Some((parts, _)) = shot.drawing {
            despawn_all(world, &parts);
        }
        world.resource_mut::<Assets<Image>>().remove(&shot.image);
    }
}

/// Despawn `entities` that are still there.
fn despawn_all(world: &mut World, entities: &[Entity]) {
    for &entity in entities {
        if let Ok(entity) = world.get_entity_mut(entity) {
            entity.despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Board, CrabKind, Direction, Handedness, Spawner, TileKind};

    /// The picture a tile kind's own sprite is drawn from, if it has one:
    /// the sand under an empty tile is the board's quad.
    fn drawn_with(art: &Art, kind: TileKind) -> Option<Handle<Image>> {
        match kind {
            TileKind::Empty => None,
            TileKind::Rock => Some(art.rock.clone()),
            TileKind::Castle(_) => Some(art.castle.clone()),
            TileKind::Spawner(_) => Some(art.hole.clone()),
            TileKind::Turnstile { .. } => Some(art.log.clone()),
            TileKind::Kelp => Some(art.kelp.clone()),
            TileKind::Pool => Some(art.ripple.clone()),
        }
    }

    /// A world that draws previews, with art whose pictures tell apart.
    fn world_with_art() -> World {
        let mut world = World::new();
        world.insert_resource(Assets::<Image>::default());
        world.init_resource::<MapPreviews>();
        let mut art = Art::blank();
        let mut images = world.resource_mut::<Assets<Image>>();
        for handle in [
            &mut art.rock,
            &mut art.castle,
            &mut art.hole,
            &mut art.log,
            &mut art.kelp,
            &mut art.ripple,
            &mut art.crab,
            &mut art.gull,
            &mut art.sign_board,
        ] {
            *handle = images.add(Image::default());
        }
        world.insert_resource(art);
        world
    }

    /// The pictures a preview's sprites are drawn from.
    fn pictures(world: &mut World) -> Vec<Handle<Image>> {
        world
            .query_filtered::<&Sprite, With<PreviewPart>>()
            .iter(world)
            .map(|sprite| sprite.image.clone())
            .collect()
    }

    /// A card's picture shows everything the round shows on its board:
    /// every tile kind's own sprite, the logs at their lean, the crabs,
    /// the gulls and the posts, by the round's own spawners. The logs were
    /// missing, being drawn live by the round alone. And nothing of it is
    /// left for the round's systems to find and redraw.
    #[test]
    fn a_preview_draws_every_piece_the_board_has() {
        let mut world = world_with_art();
        let mut board = Board::new(8, 6, 1);
        let tiles = [
            (0, 0, TileKind::Castle(0)),
            (7, 0, TileKind::Castle(1)),
            (2, 1, TileKind::Rock),
            (3, 1, TileKind::Kelp),
            (4, 2, TileKind::Pool),
            (
                5,
                2,
                TileKind::Spawner(Spawner {
                    dir: Direction::Left,
                    period: 60,
                }),
            ),
            (2, 3, TileKind::Turnstile { next_right: true }),
            (3, 3, TileKind::Turnstile { next_right: false }),
        ];
        for (x, y, kind) in tiles {
            board.set_tile(x, y, kind);
        }
        board.spawn_crab(1, 4, Direction::Right, Handedness::Left, CrabKind::Common);
        board.spawn_gull(6, 4, Direction::Up);
        assert!(board.place_signpost(0, 1, 5, Direction::Up));
        draw_card(
            &mut world,
            0,
            Pic::Card(Card::Map(MapChoice::GenSmall)),
            Shows::of(&MatchConfig::default(), 1, false),
            &board,
        );
        let drawn = pictures(&mut world);
        let art = world.resource::<Art>();
        for (x, y, kind) in board.tiles() {
            if let Some(picture) = drawn_with(art, kind) {
                assert!(drawn.contains(&picture), "{kind:?} at {x},{y}");
            }
        }
        let count = |picture: &Handle<Image>| drawn.iter().filter(|d| *d == picture).count();
        assert_eq!(count(&art.log), 2, "both logs");
        assert_eq!(count(&art.crab), 1, "the crab");
        assert_eq!(count(&art.gull), 1, "the gull");
        assert_eq!(count(&art.sign_board), 1, "the post");
        let live = world
            .query_filtered::<Entity, Or<(
                With<crate::app::board_render::TurnstileSprite>,
                With<crate::app::board_render::CastleSprite>,
                With<crate::app::board_render::SignpostSprite>,
                With<crate::app::creatures::CrabSprite>,
                With<crate::app::creatures::GullSprite>,
            )>>()
            .iter(&world)
            .count();
        assert_eq!(live, 0, "a still picture, which no round system redraws");
    }

    /// And through the cards themselves: a handmade beach with its logs
    /// either way round gets them on its card's picture.
    #[test]
    fn a_beach_with_logs_shows_them_on_its_card() {
        let mut world = world_with_art();
        world.insert_resource(State::new(Screen::MatchSetup));
        world.insert_resource(MatchMenu {
            step: Step::Map,
            seed: 3,
            ..MatchMenu::default()
        });
        world.insert_resource(MatchConfig::default());
        let level = crate::sim::Level::parse(
            "name: Logs\nposts: 1\nkind: arena\ncrab: 0,1 R R common\nmap:\n\
             +-+-+-+-+\n|0 T t 1|\n+ + + + +\n|. . . .|\n+-+-+-+-+\n",
        )
        .expect("a level");
        world.insert_resource(CustomBeaches(vec![Beach::new(level)]));
        // A few cards a frame: the shelf's is the seventh.
        for _ in 0..4 {
            render_previews(&mut world);
        }
        let held = world.resource::<MapPreviews>().drawing();
        assert!(held > 0);
        let logs = {
            let art = world.resource::<Art>().log.clone();
            pictures(&mut world).iter().filter(|d| **d == art).count()
        };
        assert!(logs >= 2, "the beach's two logs, at least: {logs}");
    }
}
