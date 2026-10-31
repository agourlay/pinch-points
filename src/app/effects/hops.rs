//! A banked crab's last hop into the keep, and the cheer it gives there.

use super::*;

/// A crab on its last quarter-second: the sim has stopped counting it, and
/// the render layer walks it the rest of the way into the keep.
///
/// The bank is the game's most-repeated reward, so the crab is seen to
/// arrive rather than disappearing under a puff.
#[derive(Component)]
pub struct Hop {
    from: Vec2,
    pub(super) to: Vec2,
    /// Seconds since it set off; below zero while it waits its turn.
    pub(super) age: f32,
    /// Its shell colour, which a crab waiting to be swept glows away from.
    shell: Color,
    /// Which way it goes home, which sets how long it is in the air and
    /// the arc it flies.
    pub(super) path: HopPath,
    /// Whether it pumps its claws on the doorstep before it hops in (see
    /// [`Homeward::Cheer`]): its age starts [`CHEER`] below zero.
    cheer: bool,
    /// Which side its big claw is on, which way the pump swings it.
    handed: crate::sim::Handedness,
    /// Whether its keep bounces when it lands: a crab that cost its owner
    /// points (a left claw under Right Claws) lands without a celebration.
    kicks: bool,
}

/// One of a hopping crab's claws, its body's child: the big one it always
/// has, or the small one it raises only while it cheers.
#[derive(Component)]
pub struct HopClaw {
    big: bool,
}

/// How a banked crab goes into its keep.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Homeward {
    /// Walked to the door and the first to bank there: it pumps both claws
    /// twice, bracing, before the hop.
    Cheer,
    /// Banked for nothing, or flown home by the tide and already at the
    /// gate: straight over the wall.
    Hop,
    /// Walked to the door behind one still waiting there to go in: it waits
    /// this many seconds on the doorstep, then goes straight over the wall,
    /// landing after the crab ahead of it. The sim has scored it already,
    /// so it does not cheer, and a castle bounces as each crab lands in it,
    /// so it must not land first.
    Behind(f32),
    /// Thrown home from across the beach by the tide, setting off this many
    /// slots after the first of its sweep.
    Swept(usize),
}

/// How long a crab cheers on the doorstep, and how many times it pumps
/// its claws in that time. Short, and the score pip is already up: the
/// cheer is the crab's, not the news.
const CHEER: f32 = 0.45;
const CHEER_PUMPS: f32 = 2.0;

/// How far a cheering crab's big claw swings out, in radians, how much it
/// grows, how far it pushes forward (world units), and how much the body
/// braces under it, all at the top of a pump.
const CHEER_SWING: f32 = 24.0 * std::f32::consts::PI / 180.0;
const CHEER_CLAW_GROW: f32 = 0.28;
const CHEER_PUSH: f32 = 3.2;
const CHEER_BRACE: f32 = 0.05;

/// Where a crab's claw roots on its body, as a fraction of its sprite's
/// side: forward and to the claw's side (the art's shoulder). The pump
/// swings and scales the claw about this point, so it stays on the crab.
const CLAW_ROOT: Vec2 = Vec2::new(0.22, 0.24);

/// How hard a cheering crab is pumping `cheered` seconds into its cheer:
/// 0 at rest, 1 at the top of a pump, [`CHEER_PUMPS`] humps over
/// [`CHEER`], each sharpened so the claws snap up and hang a moment.
fn pump(cheered: f32) -> f32 {
    if !(0.0..CHEER).contains(&cheered) {
        return 0.0;
    }
    let u = cheered / CHEER;
    (u * CHEER_PUMPS * std::f32::consts::PI)
        .sin()
        .abs()
        .powf(0.7)
}

/// Where a hopping crab's claw sits on its body at `pump`: swung out about
/// its root, grown and pushed forward, in the body's own frame (x along
/// its heading). `big` is the hand's claw; the small one is the other side
/// at 0.62 of the size, swinging the other way. `mirror` is a left hand.
fn claw_pose(side: f32, big: bool, mirror: bool, pump: f32) -> Transform {
    // Which side of the body it is on: the big claw is on the crab's
    // right (y down in its frame) unless it is mirrored, the small one on
    // the other side. Each swings out away from the body.
    let across = if big != mirror { -1.0 } else { 1.0 };
    let root = Vec2::new(CLAW_ROOT.x, CLAW_ROOT.y * across) * side;
    let swing = CHEER_SWING * pump * across;
    let (grow, push) = if big {
        (1.0 + CHEER_CLAW_GROW * pump, CHEER_PUSH * pump)
    } else {
        (0.62 * (1.0 + 0.25 * pump), CHEER_PUSH * 0.8 * pump)
    };
    let turn = Quat::from_rotation_z(swing);
    let held = root + (turn * (-root * grow).extend(0.0)).truncate() + Vec2::new(push, 0.0);
    Transform::from_translation(held.extend(CLAW_LIFT))
        .with_rotation(turn)
        .with_scale(Vec3::splat(grow))
}

/// How far over its body a hopping crab's claws are drawn, as
/// `creatures::crab_claw` lifts a walking crab's.
const CLAW_LIFT: f32 = 0.01;

/// How a banked crab gets into its keep.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum HopPath {
    /// Walked there: a short hop over the wall, off at once.
    OverWall,
    /// The tide threw it home from across the beach, on [`sweep_arc`].
    /// `waiting` is how long it has waited for the board to be clear,
    /// while it is still on the sand; it kicks up the sand it stood on
    /// when it goes, and then it is `None`.
    Swept { waiting: Option<f32> },
}

impl HopPath {
    /// Seconds in the air.
    fn span(self) -> f32 {
        match self {
            HopPath::OverWall => HOP,
            HopPath::Swept { .. } => SWEEP_HOP,
        }
    }
}

/// How long a crab is in the air on its way into the keep. Short: the
/// score has already moved, and a slow hop would read as lag.
const HOP: f32 = 0.19;

/// How high a crab hopping over the wall rises, in world units.
const HOP_LIFT: f32 = 12.0;

/// Where a crab waiting to be swept home is drawn: over the crabs round it,
/// since it is the one the banner is talking about.
pub(crate) const SWEEP_WAIT_Z: f32 = layout::z::CREATURE + 0.3;

/// Where a crab the tide throws home is, `progress` of the way from where
/// it stood to the keep, and how big it looks.
///
/// One arc for both ways a sweep is drawn: flown by the sim under
/// `sweep_flight` ([`crate::app::creatures::draw_sweeps`]) and hopped by
/// the render layer on an older board ([`bank_hop`]). Each kept its own
/// copy once, and the older one rose without limit and flew under the
/// fences.
pub(crate) fn sweep_arc(from: Vec2, to: Vec2, progress: f32) -> (Vec3, f32) {
    // Higher the further it goes, but never off the top of the beach: a
    // crab thrown corner to corner left the board at a third of it.
    let lift = (from.distance(to) * 0.35).min(TILE * 1.2);
    let (at, scale) = arc(from, to, lift, progress);
    // Over the fences while it is in the air.
    (at.extend(layout::z::WALL + 0.3), scale)
}

/// How long a crab the tide swept home is in the air, and how far behind
/// the one before it each sets off.
///
/// Crab Monopoly banks half the beach from wherever the crabs stand. At
/// [`HOP`] a crab five tiles out crossed the board in a fifth of a second,
/// shrinking as it went, and the whole sweep read as crabs blinking out:
/// the event a player had just been told about was nowhere to be seen. In
/// the air long enough to follow, one after another, it is a sweep.
const SWEEP_HOP: f32 = 0.75;
const SWEEP_STAGGER: f32 = 0.08;

/// The swept crabs wait, glowing, while a banner holds the middle of the
/// board ([`Announcer::holds_the_board`]): the event's own, and any queued
/// ahead of it. Flown at once, every crab in the middle rows went home
/// under it. They wait at least long enough for the event's banner to be
/// queued, which can be a frame after the sweep is seen, and never longer
/// than the most banners a queue can show.
///
/// [`Announcer::holds_the_board`]: crate::app::announce::Announcer::holds_the_board
const SWEEP_MIN_HOLD: f32 = 0.2;
const SWEEP_MAX_HOLD: f32 = 8.0;

/// Farther than any crab walking in is from the keep it walks into: a
/// bank from here was the tide's doing.
const SWEPT_FROM: f32 = TILE * 1.5;

/// Farther than this from its keep, a banked crab is on the doorstep, a
/// tile out: nearer, the tide has already flown it to the gate, and it has
/// no doorstep to cheer on.
const ON_THE_DOORSTEP: f32 = TILE * 0.25;

/// How long after the crab ahead of it sets off over the wall a crab
/// queued at the same door sets off: under a hop, so the two land in turn.
const QUEUE_GAP: f32 = 0.12;

/// How a crab the sim banked from `pos` into `keep`, worth `points`, goes
/// in. `sweeps` counts the crabs the tide has thrown home so far this
/// frame, and `doorsteps` holds, per keep, how long until the last crab
/// waiting at its door sets off; both are brought up to date.
///
/// The first crab at a door cheers there if it walked to the doorstep and
/// scored. One arriving while another still waits at that door queues
/// behind it and goes in after it: the castle bounces as each lands, and
/// it must not bounce for the second while the first is still outside.
pub(super) fn homeward(
    pos: Vec2,
    keep: Vec2,
    points: i32,
    sweeps: &mut usize,
    doorsteps: &mut Vec<(Vec2, f32)>,
) -> Homeward {
    let out = pos.distance(keep);
    if out > SWEPT_FROM {
        *sweeps += 1;
        return Homeward::Swept(*sweeps - 1);
    }
    if let Some((_, wait)) = doorsteps.iter_mut().find(|(at, _)| at.distance(keep) < 1.0) {
        *wait += QUEUE_GAP;
        return Homeward::Behind(*wait);
    }
    if points > 0 && out > ON_THE_DOORSTEP {
        doorsteps.push((keep, CHEER));
        return Homeward::Cheer;
    }
    Homeward::Hop
}

/// A crab the sim has just banked, as its hop home needs it: which crab,
/// from the tile it vanished on to the keep it vanished into.
pub struct BankedCrab {
    pub id: u32,
    pub from: Vec2,
    pub to: Vec2,
    pub kind: crate::sim::CrabKind,
    pub handed: crate::sim::Handedness,
    /// Whether its claw wears the seat colour of a co-op stage
    /// ([`crate::app::creatures::claw_color`]), as it did walking there.
    pub coop: bool,
    /// What it did to its owner's score.
    pub points: i32,
}

/// Send a banked crab home over the wall: the crab that walked there, big
/// claw and all, on the side it was on, facing the keep. The first at its
/// door cheers there first ([`Homeward`]); one the tide swept home from
/// across the beach is flung there instead.
pub fn bank_hop(commands: &mut Commands, art: &Art, crab: &BankedCrab, way: Homeward) {
    use crate::app::creatures::{
        claw_color, crab_body, mirrored, shade_of, shell_color, sprite_side,
    };
    let BankedCrab {
        id,
        from,
        to,
        kind,
        handed,
        coop,
        points,
    } = *crab;
    let shell = shell_color(kind, shade_of(id));
    let (age, path, z) = match way {
        Homeward::Cheer => (-CHEER, HopPath::OverWall, layout::z::CREATURE + 0.1),
        Homeward::Hop => (0.0, HopPath::OverWall, layout::z::CREATURE + 0.1),
        // Under the one ahead of it, which it stands behind.
        Homeward::Behind(wait) => (-wait, HopPath::OverWall, layout::z::CREATURE + 0.05),
        Homeward::Swept(slot) => (
            -SWEEP_STAGGER * slot as f32,
            HopPath::Swept { waiting: Some(0.0) },
            SWEEP_WAIT_Z,
        ),
    };
    let cheer = way == Homeward::Cheer;
    let hop = Hop {
        from,
        to,
        age,
        shell,
        path,
        cheer,
        handed,
        kicks: points > 0,
    };
    // Facing the keep it walked to, as it walked: drawn square to the
    // board, a crab that came in from the right turned its back on its
    // own castle for the hop.
    let toward = to - from;
    let facing = match toward.length_squared() > 1.0 {
        true => Quat::from_rotation_z(toward.to_angle()),
        false => Quat::IDENTITY,
    };
    let side = sprite_side(kind);
    let mirror = mirrored(handed);
    let claw = |big: bool, color: Color| {
        (
            HopClaw { big },
            Sprite {
                image: art.claw.clone(),
                color,
                custom_size: Some(Vec2::splat(side)),
                // The small claw is the big one's art on the other side.
                flip_y: mirror == big,
                ..default()
            },
            claw_pose(side, big, mirror, 0.0),
        )
    };
    let mut body = commands.spawn((
        hop,
        crab_body(art, kind, handed, shell),
        Transform::from_translation(from.extend(z)).with_rotation(facing),
    ));
    body.with_child(claw(true, claw_color(handed, coop)));
    if cheer {
        body.with_child(claw(false, shell.with_alpha(0.0)));
    }
}

/// A banked crab under reduced motion: no hop over the wall, but not gone
/// on the spot either, which is what a crab eaten looks like with the
/// feathers taken away. It fades where it stood, claw and all, on the
/// doorstep of the castle whose number floats up beside it.
///
/// The claw is its own particle rather than the body's child, so each
/// fades and despawns itself: a child would hold its colour to the end.
pub(super) fn bank_fade(commands: &mut Commands, art: &Art, crab: &BankedCrab) {
    use crate::app::creatures::{crab_body, crab_claw, shade_of, shell_color, sprite_side};
    let BankedCrab {
        id,
        from,
        to: _,
        kind,
        handed,
        coop,
        points: _,
    } = *crab;
    let shell = shell_color(kind, shade_of(id));
    let z = layout::z::CREATURE + 0.1;
    commands.spawn((
        standing_fade(),
        crab_body(art, kind, handed, shell),
        Transform::from_translation(from.extend(z)),
    ));
    let (claw, lift) = crab_claw(art, handed, coop, Vec2::splat(sprite_side(kind)));
    commands.spawn((
        standing_fade(),
        claw,
        Transform::from_translation(from.extend(z) + lift.translation),
    ));
}

/// A particle that stays exactly where and as big as it was spawned and
/// only fades: the reduced-motion shape of a thing leaving the board.
pub(super) fn standing_fade() -> Particle {
    Particle {
        fade_in: 0.0,
        life: 0.5,
        ..default()
    }
}

/// Where a banking crab is, and how big it looks, `progress` of the way
/// from the tile it vanished on to the keep it vanished into.
///
/// The two ends are the whole of it: a crab that starts anywhere but where
/// it was last drawn jumps on its first frame, and one that finishes
/// anywhere but at the gate is last seen standing on the wall.
pub(crate) fn arc(from: Vec2, to: Vec2, lift: f32, progress: f32) -> (Vec2, f32) {
    // Eased both ends, so it sets off and arrives rather than being
    // dragged across.
    let eased = progress * progress * (3.0 - 2.0 * progress);
    // Over the wall: zero lift at both ends, so the path meets the sand at
    // one end and the gate at the other.
    let lift = (progress * std::f32::consts::PI).sin() * lift;
    // Shrinking into the gate: the keep is a hole in the picture as far as
    // the crab is concerned.
    (
        from.lerp(to, eased) + Vec2::new(0.0, lift),
        1.0 - 0.7 * eased,
    )
}

/// Carry the hopping crabs over the wall and puff them out of sight, the
/// first at its door cheering there first, and bounce each keep as its
/// crab lands in it: a castle that bounced while its crab still stood on
/// the doorstep celebrated a crab that had not come in yet.
///
/// Held, not run, under a pause card: a crab does not go on over the wall
/// of a frozen beach. The round's end does not hold them: the crab banked
/// on the last tick still lands under the card.
#[allow(clippy::too_many_arguments)]
pub fn advance_hops(
    time: Res<Time>,
    mut commands: Commands,
    art: Res<Art>,
    mut rng: ResMut<VisualRng>,
    announcer: Res<crate::app::announce::Announcer>,
    round: crate::app::round_state::RoundState,
    sim: Res<crate::app::Sim>,
    mut hops: Query<(Entity, &mut Hop, &mut Transform, &mut Sprite, &Children)>,
    mut claws: Query<(&HopClaw, &mut Transform, &mut Sprite), Without<Hop>>,
    mut castles: Query<(
        &crate::app::board_render::CastleSprite,
        &mut crate::app::board_render::CastleKick,
    )>,
) {
    if round.held() {
        return;
    }
    let dt = time.delta_secs();
    for (entity, mut hop, mut transform, mut sprite, children) in &mut hops {
        let hop = &mut *hop;
        if let HopPath::Swept {
            waiting: Some(waited),
        } = &mut hop.path
        {
            // Picked out while the banner says what is about to happen.
            let glow = 0.45 + 0.35 * (*waited * 9.0).sin();
            layout::tint(&mut sprite, hop.shell.mix(&palette::GOLD, glow));
            let held = *waited < SWEEP_MIN_HOLD || announcer.holds_the_board();
            if held && *waited < SWEEP_MAX_HOLD {
                *waited += dt;
                continue;
            }
            // Then one after another, each its turn behind the last.
            hop.age += dt;
            if hop.age < 0.0 {
                continue;
            }
            hop.path = HopPath::Swept { waiting: None };
            layout::tint(&mut sprite, hop.shell);
            burst(
                &mut commands,
                &mut rng,
                &Burst {
                    image: art.puff.clone(),
                    pos: hop.from,
                    color: SAND_KICK,
                    count: 4,
                    size: 12.0,
                    speed: 36.0,
                    gravity: 30.0,
                },
            );
        } else {
            hop.age += dt;
        }
        let progress = (hop.age / hop.path.span()).clamp(0.0, 1.0);
        if progress >= 1.0 {
            commands.entity(entity).despawn();
            if hop.kicks {
                crate::app::board_render::kick_keep(&sim.0, &mut castles, hop.to);
            }
            // The sand it kicks up going over the wall, landing where it
            // landed rather than where it left.
            burst(
                &mut commands,
                &mut rng,
                &Burst {
                    image: art.puff.clone(),
                    pos: hop.to,
                    color: Color::srgba(0.95, 0.88, 0.7, 1.0),
                    count: 4,
                    size: 14.0,
                    speed: 40.0,
                    gravity: 40.0,
                },
            );
            continue;
        }
        match hop.path {
            HopPath::Swept { .. } => {
                let (at, scale) = sweep_arc(hop.from, hop.to, progress);
                transform.translation = at;
                transform.scale = Vec3::splat(scale);
            }
            HopPath::OverWall => {
                let (at, scale) = arc(hop.from, hop.to, HOP_LIFT, progress);
                transform.translation.x = at.x;
                transform.translation.y = at.y;
                transform.scale = Vec3::splat(scale);
            }
        }
        if hop.cheer {
            cheer(hop, &mut transform, children, &mut claws);
        }
    }
}

/// Pose a cheering crab on its doorstep: both claws up and out on each
/// pump, the body bracing under them, and everything back at rest, the
/// small claw gone, once the hop begins.
fn cheer(
    hop: &Hop,
    body: &mut Transform,
    children: &Children,
    claws: &mut Query<(&HopClaw, &mut Transform, &mut Sprite), Without<Hop>>,
) {
    let pumping = pump(hop.age + CHEER);
    if hop.age < 0.0 {
        body.scale = Vec3::splat(1.0 + CHEER_BRACE * pumping);
    }
    let mirror = crate::app::creatures::mirrored(hop.handed);
    for child in children {
        let Ok((claw, mut place, mut sprite)) = claws.get_mut(*child) else {
            continue;
        };
        let side = sprite.custom_size.map_or(TILE, |size| size.x);
        let pose = claw_pose(side, claw.big, mirror, pumping);
        place.translation = pose.translation;
        place.rotation = pose.rotation;
        place.scale = pose.scale;
        if !claw.big {
            let pumped = sprite.color.with_alpha(pumping);
            layout::tint(&mut sprite, pumped);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::stage;
    use super::*;

    /// A pump is two humps over the cheer, at rest at both ends, and the
    /// claw it swings stays rooted on the body: the root is where it was
    /// at rest however hard the crab pumps.
    #[test]
    fn a_cheer_pumps_twice_about_the_claws_root() {
        assert_eq!(pump(0.0), 0.0);
        assert!(pump(CHEER - 1e-4) < 0.05, "at rest as the hop begins");
        assert_eq!(pump(CHEER + 0.1), 0.0);
        assert!((pump(CHEER * 0.25) - 1.0).abs() < 1e-3, "the first pump");
        assert!((pump(CHEER * 0.75) - 1.0).abs() < 1e-3, "the second");
        assert!(pump(CHEER * 0.5) < 0.01, "and down between");
        let side = 40.0;
        for (big, mirror) in [(true, false), (true, true), (false, false), (false, true)] {
            let rest = claw_pose(side, big, mirror, 0.0);
            let up = claw_pose(side, big, mirror, 1.0);
            let across = if big != mirror { -1.0 } else { 1.0 };
            let root = Vec2::new(CLAW_ROOT.x, CLAW_ROOT.y * across) * side;
            let push = if big { CHEER_PUSH } else { CHEER_PUSH * 0.8 };
            // Where each pose puts the point of the art that is its root,
            // less the push forward that carries the whole claw.
            let at = |pose: Transform, local: Vec2| {
                (pose.translation + pose.rotation * (local * pose.scale.x).extend(0.0)).truncate()
            };
            let rest_root = at(rest, root);
            let up_root = at(up, root) - Vec2::new(push, 0.0);
            assert!(
                rest_root.distance(up_root) < 0.5,
                "big {big} mirror {mirror}: {rest_root} vs {up_root}"
            );
            // Out, away from the body: the claw on the right turns clockwise.
            assert_eq!(up.rotation.to_euler(EulerRot::XYZ).2.signum(), across);
        }
    }

    /// A banking crab has to leave from exactly where it was last drawn
    /// and arrive at exactly the gate, and be off the ground in between,
    /// which is the only thing saying it went *over* rather than
    /// *through*.
    #[test]
    fn a_bank_starts_on_the_sand_and_finishes_in_the_gate() {
        let (from, to) = (Vec2::new(-120.0, 30.0), Vec2::new(64.0, 64.0));
        let (start, start_scale) = arc(from, to, 12.0, 0.0);
        assert_eq!(start, from);
        assert!((start_scale - 1.0).abs() < 1e-6, "full size on the sand");
        let (end, end_scale) = arc(from, to, 12.0, 1.0);
        assert!(end.distance(to) < 1e-3, "{end:?} vs {to:?}");
        assert!(end_scale < 0.35, "and all but gone: {end_scale}");
        let (middle, _) = arc(from, to, 12.0, 0.5);
        assert!(
            middle.y > from.lerp(to, 0.5).y,
            "off the ground in between: {middle:?}"
        );
    }

    /// A banking crab travels straight to the gate, never bowing sideways
    /// off it: a path that bowed would carry the crab over a wall on a
    /// corner-to-corner bank, through terrain it never touched.
    #[test]
    fn a_bank_travels_in_a_straight_line() {
        let (from, to) = (Vec2::new(0.0, 0.0), Vec2::new(400.0, 200.0));
        for step in 0..=10 {
            let (at, _) = arc(from, to, 12.0, step as f32 / 10.0);
            // Every point is on the segment once the lift is taken back
            // off: y is exactly half of x, which is what the two ends say.
            let lift = (step as f32 / 10.0 * std::f32::consts::PI).sin() * 12.0;
            assert!(
                (at.y - lift - at.x * 0.5).abs() < 1e-3,
                "bowed off the line at {step}: {at:?}"
            );
        }
    }

    /// A crab the tide throws corner to corner stays on the beach and over
    /// the fences the whole way, whichever of the two sweeps drew it.
    #[test]
    fn a_long_sweep_stays_under_the_cap_and_over_the_walls() {
        let (from, to) = (Vec2::new(-400.0, -300.0), Vec2::new(400.0, 300.0));
        for step in 0..=20 {
            let progress = step as f32 / 20.0;
            let (at, _) = sweep_arc(from, to, progress);
            let (flat, _) = arc(from, to, 0.0, progress);
            assert!(
                at.y - flat.y <= TILE * 1.2 + 1e-3,
                "too high at {step}: {at:?}"
            );
            assert!(at.z > layout::z::WALL, "under the fences at {step}");
        }
    }

    /// A crab banked at `keep` from beside it, worth `points`.
    fn banked_at(id: u32, keep: Vec2, points: i32) -> SimEvent {
        SimEvent::CrabBanked {
            id,
            owner: 0,
            pos: keep + Vec2::new(TILE * 0.5, 0.0),
            keep,
            points,
            kind: crate::sim::CrabKind::Common,
            handed: crate::sim::Handedness::Right,
        }
    }

    /// The cheering crabs on the beach, and the ones hopping straight in.
    fn cheers(app: &mut App) -> (usize, usize) {
        let world = app.world_mut();
        let hops: Vec<bool> = world.query::<&Hop>().iter(world).map(|h| h.cheer).collect();
        let cheering = hops.iter().filter(|&&c| c).count();
        (cheering, hops.len() - cheering)
    }

    /// Only the first crab at a door cheers there. Two banking at one keep
    /// on one frame, or one a frame behind a crab still cheering, go
    /// straight in; a crab at another keep cheers for its own, and one
    /// that cost its owner points does not cheer at all.
    #[test]
    fn only_the_first_crab_at_a_door_cheers() {
        let mut app = stage(1, false);
        app.add_systems(Update, moment_effects);
        let keep = Vec2::new(100.0, 0.0);
        app.world_mut().write_message(banked_at(1, keep, 1));
        app.world_mut().write_message(banked_at(2, keep, 1));
        app.update();
        assert_eq!(cheers(&mut app), (1, 1), "one cheers, one goes in");
        app.world_mut().write_message(banked_at(3, keep, 1));
        app.world_mut().write_message(banked_at(4, -keep, 1));
        app.world_mut().write_message(banked_at(5, keep * 3.0, -2));
        app.update();
        assert_eq!(cheers(&mut app), (2, 3), "the other keep cheers its own");
    }

    /// How long until each crab hopping into `keep` lands, soonest first.
    fn landings(app: &mut App, keep: Vec2) -> Vec<f32> {
        let world = app.world_mut();
        let mut out: Vec<f32> = world
            .query::<&Hop>()
            .iter(world)
            .filter(|hop| hop.to == keep)
            .map(|hop| hop.path.span() - hop.age)
            .collect();
        out.sort_by(f32::total_cmp);
        out
    }

    /// A crab banking at a door while another still cheers on its doorstep
    /// waits behind it and lands after it, on the same frame or a later
    /// one, and each lands on its own beat: the castle bounces as each
    /// crab lands, and must not for the second while the first is outside.
    #[test]
    fn a_crab_behind_a_cheering_one_lands_after_it() {
        let mut app = stage(1, false);
        app.add_systems(Update, moment_effects);
        let keep = Vec2::new(100.0, 0.0);
        app.world_mut().write_message(banked_at(1, keep, 1));
        app.world_mut().write_message(banked_at(2, keep, 1));
        app.update();
        app.world_mut().write_message(banked_at(3, keep, 1));
        app.update();
        let lands = landings(&mut app, keep);
        assert_eq!(lands.len(), 3, "{lands:?}");
        assert!(
            (lands[0] - (CHEER + HOP)).abs() < 1e-5,
            "the cheering crab lands first: {lands:?}"
        );
        for pair in lands.windows(2) {
            assert!(
                pair[1] - pair[0] >= QUEUE_GAP - 1e-5,
                "one at a time: {lands:?}"
            );
        }
    }

    /// A crab the tide has already flown to its gate banks there, with no
    /// doorstep under it: it goes straight in rather than pumping its claws
    /// on the castle's roof.
    #[test]
    fn a_crab_flown_to_its_gate_does_not_cheer_on_the_roof() {
        let mut app = stage(1, false);
        app.add_systems(Update, moment_effects);
        let keep = Vec2::new(100.0, 0.0);
        app.world_mut().write_message(SimEvent::CrabBanked {
            id: 1,
            owner: 0,
            pos: keep,
            keep,
            points: 1,
            kind: crate::sim::CrabKind::Common,
            handed: crate::sim::Handedness::Right,
        });
        app.update();
        assert_eq!(cheers(&mut app), (0, 1), "straight in");
        app.world_mut().write_message(banked_at(2, keep, 1));
        app.update();
        assert_eq!(cheers(&mut app), (1, 1), "and the next walking in cheers");
    }
}
