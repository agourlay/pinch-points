//! A cup's schedule: which entrants sit at which table on which beach, and
//! in which chairs.
//!
//! - **Rotation, not permutation.** At each table, on each beach, the seats
//!   rotate so every entrant sits in every chair once: seat bias is real,
//!   and a rotation is what cancels it.
//! - **Tables are drawn, not enumerated.** Every possible table does not
//!   scale, so tables are drawn so that every entrant plays as often as
//!   every other and every pair shares a table as evenly as the count of
//!   games allows. The draw is seeded and printed, so it can be checked.
//! - **One owner per table.** Two bots of one owner never sit together:
//!   copies at one table can play as a team. If the field is too small to
//!   keep them apart, the draw refuses and says why.

use crate::sim::Pcg32;

/// One game of a cup: the beach it is on, and who sits in each chair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixture {
    /// Which of the cup's beaches, from 0.
    pub beach: usize,
    /// Entrant indices, chair by chair.
    pub chairs: Vec<usize>,
}

/// Draw the cup: `beaches` beaches, `tables` tables on each, `seats`
/// chairs a table. `owners[i]` is entrant `i`'s owner, if it has one (the
/// game's AI has none).
pub fn draw(
    owners: &[Option<String>],
    seats: usize,
    beaches: usize,
    tables: usize,
    seed: u64,
) -> Result<Vec<Fixture>, String> {
    let n = owners.len();
    if n < seats {
        return Err(format!("{n} entrants cannot fill a {seats}-seat table"));
    }
    let same_owner = |a: usize, b: usize| matches!((&owners[a], &owners[b]), (Some(x), Some(y)) if x.eq_ignore_ascii_case(y));
    let mut rng = Pcg32::new(seed, 0xC0_FFEE);
    let mut played = vec![0u32; n];
    let mut met = vec![vec![0u32; n]; n];
    let mut fixtures = Vec::new();
    for beach in 0..beaches {
        for _ in 0..tables {
            let mut table: Vec<usize> = Vec::with_capacity(seats);
            while table.len() < seats {
                // The fewest games so far first, then the fewest meetings
                // with whoever is already at the table, then the dice.
                let pick = (0..n)
                    .filter(|&c| !table.contains(&c))
                    .filter(|&c| table.iter().all(|&t| !same_owner(c, t)))
                    .map(|c| {
                        let meetings: u32 = table.iter().map(|&t| met[c][t]).sum();
                        (played[c], meetings, rng.next_u32(), c)
                    })
                    .min();
                let Some((_, _, _, chosen)) = pick else {
                    return Err("the field is too small to keep one owner's bots apart; \
                         enter fewer bots per owner or more entrants"
                        .to_string());
                };
                table.push(chosen);
            }
            for &a in &table {
                played[a] += 1;
                for &b in &table {
                    if a != b {
                        met[a][b] += 1;
                    }
                }
            }
            // Every chair once: the table is played `seats` times, turned
            // a chair each time.
            for turn in 0..seats {
                let chairs = (0..seats)
                    .map(|chair| table[(chair + turn) % seats])
                    .collect();
                fixtures.push(Fixture { beach, chairs });
            }
        }
    }
    Ok(fixtures)
}

/// Tables a beach needs so every entrant plays once a beach, when the
/// count allows it: the fewest that seat everyone and fill a whole number
/// of times over the cup.
pub fn tables_for(entrants: usize, seats: usize, beaches: usize) -> usize {
    let least = entrants.div_ceil(seats).max(1);
    (least..=entrants.max(least))
        .find(|&t| (t * seats * beaches).is_multiple_of(entrants))
        .unwrap_or(least)
}

/// Points for a place: `seats - place` for a four-seat table is 3, 2, 1
/// and 0, and a tie splits the places it spans. `places` are 1-based
/// placings as `seat::placings` gives them.
pub fn points(places: &[u32]) -> Vec<f64> {
    let seats = places.len() as f64;
    places
        .iter()
        .map(|&place| {
            let tied = places.iter().filter(|&&p| p == place).count() as f64;
            // The places a tie at `place` spans are place .. place + tied - 1.
            let first = f64::from(place);
            let span: f64 = (0..tied as u32)
                .map(|k| seats - (first + f64::from(k)))
                .sum();
            span / tied
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nobody(n: usize) -> Vec<Option<String>> {
        vec![None; n]
    }

    #[test]
    fn as_many_entrants_as_seats_is_one_table_turned_every_way() {
        let fixtures = draw(&nobody(4), 4, 2, 1, 1).expect("draws");
        assert_eq!(fixtures.len(), 8, "2 beaches, 1 table, 4 turns");
        for chair in 0..4 {
            let sat: Vec<usize> = fixtures[..4].iter().map(|f| f.chairs[chair]).collect();
            let mut sorted = sat.clone();
            sorted.sort_unstable();
            assert_eq!(
                sorted,
                vec![0, 1, 2, 3],
                "every entrant in chair {chair} once"
            );
        }
    }

    #[test]
    fn a_big_field_plays_evenly_and_meets_evenly() {
        let n = 10;
        let beaches = 20;
        let tables = tables_for(n, 4, beaches);
        let fixtures = draw(&nobody(n), 4, beaches, tables, 7).expect("draws");
        let mut games = vec![0; n];
        let mut met = vec![vec![0; n]; n];
        for f in fixtures.iter().step_by(4) {
            for &a in &f.chairs {
                games[a] += 1;
                for &b in &f.chairs {
                    if a != b {
                        met[a][b] += 1;
                    }
                }
            }
        }
        assert!(games.iter().all(|&g| g == games[0]), "{games:?}");
        let pairs: Vec<u32> = (0..n)
            .flat_map(|a| ((a + 1)..n).map(move |b| (a, b)))
            .map(|(a, b)| met[a][b])
            .collect();
        let (lo, hi) = (pairs.iter().min().copied(), pairs.iter().max().copied());
        assert!(
            hi.zip(lo).is_some_and(|(hi, lo)| hi - lo <= 2),
            "{lo:?}..{hi:?}"
        );
    }

    #[test]
    fn one_owners_bots_never_share_a_table_and_a_field_too_small_is_refused() {
        let owners = vec![
            Some("ana".to_string()),
            Some("Ana".to_string()),
            Some("bo".to_string()),
            None,
            None,
        ];
        let fixtures = draw(&owners, 4, 5, 2, 3).expect("draws");
        for f in &fixtures {
            assert!(!(f.chairs.contains(&0) && f.chairs.contains(&1)), "{f:?}");
        }
        let crowded = vec![Some("ana".to_string()); 4];
        assert!(draw(&crowded, 4, 1, 1, 1).is_err());
    }

    #[test]
    fn places_pay_seats_minus_place_and_ties_split_them() {
        assert_eq!(points(&[2, 4, 1, 3]), vec![2.0, 0.0, 3.0, 1.0]);
        assert_eq!(points(&[1, 1, 3, 4]), vec![2.5, 2.5, 1.0, 0.0]);
        assert_eq!(points(&[1, 2]), vec![1.0, 0.0]);
    }

    #[test]
    fn a_table_count_fills_whole_rounds_when_it_can() {
        assert_eq!(tables_for(4, 4, 20), 1);
        assert_eq!(tables_for(8, 4, 20), 2);
        assert_eq!((tables_for(10, 4, 20) * 4 * 20) % 10, 0);
    }
}
