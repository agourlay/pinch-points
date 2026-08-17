//! The procedural sprite registry, loaded once at app construction and
//! shared by every rendering module. White/light shapes are tinted at
//! spawn; gulls, rocks, holes, and terrain bake their palette.

use crate::app::cycle::Cycle;
use crate::app::i18n::{ALL_LANGS, Lang};
use bevy::prelude::*;

/// Procedurally generated sprite art (tools/gen_sprites.py, and
/// tools/gen_flags.py for the language chips). White shapes are tinted at
/// spawn; gulls, rocks, and holes bake their palette.
#[derive(Resource)]
pub struct Art {
    pub arrow: Handle<Image>,
    pub crab: Handle<Image>,
    pub claw: Handle<Image>,
    pub gull: Handle<Image>,
    pub gull_fly: Handle<Image>,
    pub rock: Handle<Image>,
    pub hole: Handle<Image>,
    pub castle: Handle<Image>,
    pub sand_a: Handle<Image>,
    pub sand_b: Handle<Image>,
    pub shadow: Handle<Image>,
    pub plank: Handle<Image>,
    pub bracket: Handle<Image>,
    pub crown: Handle<Image>,
    pub kelp: Handle<Image>,
    pub pool: Handle<Image>,
    pub log: Handle<Image>,
    pub star: Handle<Image>,
    pub puff: Handle<Image>,
    pub foam: Handle<Image>,
    pub post: Handle<Image>,
    pub crab_b: Handle<Image>,
    pub wet: Handle<Image>,
    pub cloud: Handle<Image>,
    pub boat: Handle<Image>,
    /// One flag chip per language, in [`ALL_LANGS`] order. Read through
    /// [`Art::flag`] rather than indexed directly.
    pub flags: [Handle<Image>; ALL_LANGS.len()],
}

impl Art {
    /// The flag chip for a language. Built and read in the same
    /// [`ALL_LANGS`] order, so the two cannot drift apart.
    pub fn flag(&self, lang: Lang) -> Handle<Image> {
        self.flags[lang.index()].clone()
    }
}

impl FromWorld for Art {
    /// Built at app construction so it exists before the first state
    /// transition (the menu's OnEnter needs it immediately).
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Art {
            arrow: assets.load("sprites/arrow.png"),
            crab: assets.load("sprites/crab.png"),
            claw: assets.load("sprites/claw.png"),
            gull: assets.load("sprites/gull.png"),
            gull_fly: assets.load("sprites/gull_fly.png"),
            rock: assets.load("sprites/rock.png"),
            hole: assets.load("sprites/hole.png"),
            castle: assets.load("sprites/castle.png"),
            sand_a: assets.load("sprites/sand_a.png"),
            sand_b: assets.load("sprites/sand_b.png"),
            shadow: assets.load("sprites/shadow.png"),
            plank: assets.load("sprites/plank.png"),
            bracket: assets.load("sprites/bracket.png"),
            crown: assets.load("sprites/crown.png"),
            kelp: assets.load("sprites/kelp.png"),
            pool: assets.load("sprites/pool.png"),
            log: assets.load("sprites/log.png"),
            star: assets.load("sprites/star.png"),
            puff: assets.load("sprites/puff.png"),
            foam: assets.load("sprites/foam.png"),
            post: assets.load("sprites/post.png"),
            crab_b: assets.load("sprites/crab_b.png"),
            wet: assets.load("sprites/wet.png"),
            cloud: assets.load("sprites/cloud.png"),
            boat: assets.load("sprites/boat.png"),
            // Named by the language's settings key, so the set follows
            // ALL_LANGS without a second table to keep in step.
            flags: ALL_LANGS.map(|lang| assets.load(format!("sprites/flag_{}.png", lang.key()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A language with no flag file loads nothing: the asset server logs a
    /// miss and the settings row draws an empty gap where the chip goes.
    /// Nothing else notices, so this does. Run tools/gen_flags.py after
    /// adding a language.
    #[test]
    fn every_language_has_a_flag_on_disk() {
        for lang in ALL_LANGS {
            let path = format!("assets/sprites/flag_{}.png", lang.key());
            assert!(
                std::path::Path::new(&path).exists(),
                "no flag for {lang:?}: {path} is missing"
            );
        }
    }
}
