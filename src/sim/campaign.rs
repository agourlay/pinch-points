//! The Tide Pool campaign: embedded levels, in play order.
//!
//! Every level carries a `solution:` line that `tests/campaign.rs` replays
//! against the sim: a level that ships is a level that is provably solvable
//! within its signpost inventory.

use crate::sim::level::Level;

pub const CAMPAIGN: &[&str] = &[
    include_str!("../../levels/01_welcome_ashore.txt"),
    include_str!("../../levels/02_first_turn.txt"),
    include_str!("../../levels/03_follow_the_claw.txt"),
    include_str!("../../levels/04_corner_pocket.txt"),
    include_str!("../../levels/05_two_minds.txt"),
    include_str!("../../levels/06_signpost_splitter.txt"),
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

/// Beach Day (spec §5.2): goal-based 30-second challenge stages, the
/// original's Stage Challenge re-themed. Versus arrow rules (cap 3, evict,
/// posts fade), each validated by tests via its authored solution.
pub const CHALLENGES: &[&str] = &[
    include_str!("../../challenges/01_first_flood.txt"),
    include_str!("../../challenges/02_fork_the_flow.txt"),
    include_str!("../../challenges/03_hold_the_line.txt"),
    include_str!("../../challenges/04_golden_ticket.txt"),
    include_str!("../../challenges/05_torrent.txt"),
    include_str!("../../challenges/06_gull_storm.txt"),
    include_str!("../../challenges/07_double_gold.txt"),
    include_str!("../../challenges/08_feeding_frenzy.txt"),
];

pub fn challenge_levels() -> Vec<Level> {
    CHALLENGES
        .iter()
        .enumerate()
        .map(|(i, text)| {
            Level::parse(text).unwrap_or_else(|e| panic!("challenge stage {}: {e}", i + 1))
        })
        .collect()
}

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
