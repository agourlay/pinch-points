//! The Tide Pool campaign: embedded levels, in play order.
//!
//! Every level is replayed against the sim by `tests/it/campaign.rs`: one
//! that carries a `solution:` line is provably solvable within its signpost
//! inventory, and one that does not (the watch-levels, which grant no posts,
//! and the controls tutorial) provably solves itself.

use crate::sim::level::Level;

pub const CAMPAIGN: &[&str] = &[
    include_str!("../../levels/01_welcome_ashore.txt"),
    include_str!("../../levels/02_first_turn.txt"),
    include_str!("../../levels/03_follow_the_claw.txt"),
    include_str!("../../levels/04_corner_pocket.txt"),
    include_str!("../../levels/05_two_minds.txt"),
    include_str!("../../levels/06_undertow.txt"),
    include_str!("../../levels/07_family_outing.txt"),
    include_str!("../../levels/08_detour.txt"),
    include_str!("../../levels/09_both_lanes.txt"),
    include_str!("../../levels/10_confluence.txt"),
    include_str!("../../levels/11_break_the_loop.txt"),
    include_str!("../../levels/12_merry_go_round.txt"),
    include_str!("../../levels/13_crossroads.txt"),
    include_str!("../../levels/14_right_of_way.txt"),
    include_str!("../../levels/15_trust_the_claw.txt"),
    include_str!("../../levels/16_island_keep.txt"),
    include_str!("../../levels/17_two_doors.txt"),
    include_str!("../../levels/18_rush_hour.txt"),
    include_str!("../../levels/19_odd_couple.txt"),
    include_str!("../../levels/20_pinch_point.txt"),
    include_str!("../../levels/21_log_ride.txt"),
    include_str!("../../levels/22_second_serving.txt"),
    include_str!("../../levels/23_into_the_weeds.txt"),
    include_str!("../../levels/24_dry_feet.txt"),
    include_str!("../../levels/25_the_old_mill.txt"),
    include_str!("../../levels/26_open_ocean.txt"),
    include_str!("../../levels/27_follow_the_molt.txt"),
    include_str!("../../levels/28_twin_logs.txt"),
    include_str!("../../levels/29_golden_hour.txt"),
    include_str!("../../levels/30_hunker_down.txt"),
    include_str!("../../levels/31_crossfall.txt"),
    include_str!("../../levels/32_mill_race.txt"),
    include_str!("../../levels/33_rock_pool.txt"),
    include_str!("../../levels/34_misroute.txt"),
    include_str!("../../levels/35_pool_party.txt"),
    include_str!("../../levels/36_gull_gauntlet.txt"),
    include_str!("../../levels/37_half_measure.txt"),
    include_str!("../../levels/38_kelp_keep.txt"),
    include_str!("../../levels/39_toll_road.txt"),
    include_str!("../../levels/40_molt_gather.txt"),
    include_str!("../../levels/41_gold_rush.txt"),
    include_str!("../../levels/42_long_way_round.txt"),
    include_str!("../../levels/43_down_the_steps.txt"),
    include_str!("../../levels/44_zigzag.txt"),
    include_str!("../../levels/45_two_ladders.txt"),
    include_str!("../../levels/46_cold_feet.txt"),
    include_str!("../../levels/47_two_tides.txt"),
    include_str!("../../levels/48_the_long_stair.txt"),
    include_str!("../../levels/49_the_back_stair.txt"),
    include_str!("../../levels/50_gold_ladder.txt"),
    include_str!("../../levels/51_kelp_and_cross.txt"),
    include_str!("../../levels/52_two_giants.txt"),
    include_str!("../../levels/53_down_the_middle.txt"),
    include_str!("../../levels/54_left_behind.txt"),
    include_str!("../../levels/55_pincer.txt"),
    include_str!("../../levels/56_weed_between.txt"),
    include_str!("../../levels/57_slow_and_sure.txt"),
    include_str!("../../levels/58_the_long_shelf.txt"),
    include_str!("../../levels/59_quick_feet.txt"),
    include_str!("../../levels/60_rock_row.txt"),
    include_str!("../../levels/61_two_logs.txt"),
    include_str!("../../levels/62_over_the_pool.txt"),
    include_str!("../../levels/63_round_the_log.txt"),
    include_str!("../../levels/64_deep_water.txt"),
    include_str!("../../levels/65_two_giants_again.txt"),
    include_str!("../../levels/66_castle_on_high.txt"),
    include_str!("../../levels/67_three_ways_in.txt"),
    include_str!("../../levels/68_the_far_corner.txt"),
    include_str!("../../levels/69_weed_gate.txt"),
    include_str!("../../levels/70_last_of_the_tide.txt"),
    include_str!("../../levels/71_the_four_doors.txt"),
    include_str!("../../levels/72_four_doors_up.txt"),
    include_str!("../../levels/73_both_ways_home.txt"),
    include_str!("../../levels/74_the_left_turn.txt"),
    include_str!("../../levels/75_the_right_turn.txt"),
    include_str!("../../levels/76_the_ragged_lanes.txt"),
    include_str!("../../levels/77_one_down_three_up.txt"),
    include_str!("../../levels/78_three_up_and_a_sidestep.txt"),
    include_str!("../../levels/79_the_left_shoulder.txt"),
    include_str!("../../levels/80_the_right_shoulder.txt"),
    include_str!("../../levels/81_the_long_and_short.txt"),
    include_str!("../../levels/82_the_last_lane.txt"),
    include_str!("../../levels/83_the_second_column.txt"),
    include_str!("../../levels/84_second_column_going_up.txt"),
    include_str!("../../levels/85_the_staircase.txt"),
    include_str!("../../levels/86_the_corner.txt"),
    include_str!("../../levels/87_two_shores.txt"),
    include_str!("../../levels/88_cross_currents.txt"),
    include_str!("../../levels/89_on_its_side.txt"),
    include_str!("../../levels/90_the_pen.txt"),
    include_str!("../../levels/91_five_both_ways.txt"),
    include_str!("../../levels/92_the_fifth_column.txt"),
    include_str!("../../levels/93_five_steps.txt"),
    include_str!("../../levels/94_five_on_two_shores.txt"),
    include_str!("../../levels/95_five_currents.txt"),
    include_str!("../../levels/96_five_round_the_corner.txt"),
    include_str!("../../levels/97_five_on_their_side.txt"),
    include_str!("../../levels/98_both_shoulders.txt"),
    include_str!("../../levels/99_the_pen_and_the_sidecar.txt"),
    include_str!("../../levels/100_all_at_once.txt"),
];

/// Parse the whole campaign. Panics on a malformed level; the validation
/// test fails first, so a shipped binary never hits this.
pub fn campaign_levels() -> Vec<Level> {
    CAMPAIGN
        .iter()
        .enumerate()
        .map(|(i, text)| {
            Level::parse(text).unwrap_or_else(|e| panic!("campaign level {}: {e}", i + 1))
        })
        .collect()
}

/// The co-op stages: each needs two players, P1's arrows turning the left
/// claws and P2's the right (`hands:`). Embedded like the campaign, in play
/// order: by the arrows a stage grants, fewest first, then easiest first
/// among stages granting as many. The grant is what a tile's edge colour
/// shows, so the colours never step back down along the shelf.
/// `tests/it/coop.rs` proves each one needs both players and holds the
/// order to the grant.
///
/// A stage's clear is filed by its name ([`coop_progress_key`]), never by
/// its place on the shelf or its file's number, so the shelf can be
/// reordered and new stages slotted in without moving anyone's clears.
/// Renaming a shipped stage is what would lose them.
pub const COOP: &[&str] = &[
    include_str!("../../levels/coop/01_swap_lanes.txt"),
    include_str!("../../levels/coop/02_fork_in_the_road.txt"),
    include_str!("../../levels/coop/03_head_on.txt"),
    include_str!("../../levels/coop/04_two_each.txt"),
    include_str!("../../levels/coop/05_roundabout.txt"),
    include_str!("../../levels/coop/06_give_way.txt"),
    include_str!("../../levels/coop/07_crossing.txt"),
    include_str!("../../levels/coop/08_gull_in_the_lane.txt"),
    include_str!("../../levels/coop/09_gull_ahead.txt"),
    include_str!("../../levels/coop/10_flip_the_log.txt"),
    include_str!("../../levels/coop/11_the_log.txt"),
    include_str!("../../levels/coop/12_up_and_over.txt"),
    include_str!("../../levels/coop/13_round_the_rock.txt"),
    include_str!("../../levels/coop/14_busy_crossing.txt"),
    include_str!("../../levels/coop/15_turn_back.txt"),
    include_str!("../../levels/coop/16_cover_me.txt"),
    include_str!("../../levels/coop/17_shoo.txt"),
    include_str!("../../levels/coop/18_middle_lane.txt"),
    include_str!("../../levels/coop/19_pen_the_gull.txt"),
    include_str!("../../levels/coop/20_cover_each_other.txt"),
];

/// Parse every co-op stage. Panics on a malformed one; the tests read them
/// first, so a shipped binary never does.
///
/// Each is filed under a key of the co-op ladder's own
/// ([`coop_progress_key`]) rather than its bare name: an older build kept
/// a player's level's clear under that level's name, and a level of the
/// player's called "Give Way" would otherwise have cleared the shipped
/// one and opened the stage after it.
pub fn coop_levels() -> Vec<Level> {
    COOP.iter()
        .enumerate()
        .map(|(i, text)| {
            let mut level =
                Level::parse(text).unwrap_or_else(|e| panic!("co-op stage {}: {e}", i + 1));
            level.filing = Some(crate::sim::Filing {
                key: coop_progress_key(&level.name),
                legacy: None,
            });
            level
        })
        .collect()
}

/// The key a shipped co-op stage called `name` keeps its clear under. A
/// player's level is filed by its file (`file:...`) and a Tide Pool stage
/// by its bare name, so neither can hold this one.
pub fn coop_progress_key(name: &str) -> String {
    format!("coop:{name}")
}

/// The handmade versus maps, embedded like the campaign, in the order the
/// map dial lists them. Each is an ordinary arena level that says which
/// tables it is built for (`seats:`) and, on an open beach of identical
/// strips, how wide one is (`roll:`). `tests/it/maps.rs` plays each at
/// every table it seats and holds it to [`crate::sim::unplayable`].
pub const VERSUS_MAPS: [&str; 11] = [
    include_str!("../../levels/versus/the_gates.txt"),
    include_str!("../../levels/versus/the_moat.txt"),
    include_str!("../../levels/versus/log_jam.txt"),
    include_str!("../../levels/versus/three_lanes.txt"),
    include_str!("../../levels/versus/round_table.txt"),
    include_str!("../../levels/versus/round_the_world.txt"),
    include_str!("../../levels/versus/duel.txt"),
    include_str!("../../levels/versus/the_fountain.txt"),
    include_str!("../../levels/versus/four_doors.txt"),
    include_str!("../../levels/versus/gull_road.txt"),
    include_str!("../../levels/versus/back_to_back.txt"),
];

/// The `index`th handmade versus map ([`VERSUS_MAPS`]). Panics on a
/// malformed one; the tests read every one first, so a shipped binary
/// never does.
pub fn versus_map(index: usize) -> Level {
    Level::parse(VERSUS_MAPS[index]).unwrap_or_else(|e| panic!("versus map {index}: {e}"))
}
