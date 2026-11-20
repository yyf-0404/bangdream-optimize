//! A replacement may add uniformity, but must never remove an existing one.
//! Skill monotonicity is certified once per card/context, outside pair scans.
//! On refinement passes, every possible destination member must still belong
//! to the entry snapshot. This lets each pass compose without resurrecting a
//! membership removed by an earlier, narrower proof domain.
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(super) struct Identity {
    pub band: u32,
    pub attribute: u8,
}

pub(super) struct Guard {
    context: usize,
    identities: Vec<Identity>,
    destinations: BTreeMap<(usize, u32, u8), bool>,
}

fn key(c: Identity, context: usize) -> (usize, u32, u8) {
    (
        context,
        if context & 1 != 0 { c.band } else { 0 },
        if context & 2 != 0 { c.attribute } else { 0 },
    )
}

impl Guard {
    pub fn new(
        context: usize,
        identities: Vec<Identity>,
        indices: &[usize],
        monotone: &[[bool; 4]],
        prior: Option<&[[bool; 4]]>,
    ) -> Self {
        let mut destinations = BTreeMap::new();
        for &i in indices {
            for destination in 0..4 {
                if destination != context && destination & context == context {
                    let safe = monotone[i][destination] && prior.is_none_or(|p| p[i][destination]);
                    destinations
                        .entry(key(identities[i], destination))
                        .and_modify(|v: &mut bool| *v &= safe)
                        .or_insert(safe);
                }
            }
        }
        Self {
            context,
            identities,
            destinations,
        }
    }

    pub fn allows(&self, old: usize, new: usize) -> bool {
        let old = self.identities[old];
        let new = self.identities[new];
        let changed =
            usize::from(old.band != new.band) | (usize::from(old.attribute != new.attribute) << 1);
        if changed & self.context != 0 {
            return false;
        }
        // Check both single additions AND their conjunction. Neither single
        // transition proves a skill conditioned on band and attribute jointly.
        for addition in 1..4 {
            if addition & changed == addition
                && !self
                    .destinations
                    .get(&key(new, self.context | addition))
                    .copied()
                    .unwrap_or(false)
            {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_mixed_transitions_without_losing_existing_uniformity() {
        let cards = vec![
            Identity {
                band: 1,
                attribute: 0,
            },
            Identity {
                band: 2,
                attribute: 0,
            },
            Identity {
                band: 1,
                attribute: 1,
            },
            Identity {
                band: 2,
                attribute: 1,
            },
        ];
        for context in 0..4 {
            let guard = Guard::new(context, cards.clone(), &[0, 1, 2, 3], &[[true; 4]; 4], None);
            for j in 0..4 {
                assert_eq!(guard.allows(0, j), j & context == 0);
            }
        }
    }

    #[test]
    fn joint_upgrade_and_previously_removed_membership_need_their_own_proof() {
        let cards = vec![
            Identity {
                band: 1,
                attribute: 0,
            },
            Identity {
                band: 2,
                attribute: 1,
            },
            Identity {
                band: 2,
                attribute: 1,
            },
            Identity {
                band: 3,
                attribute: 2,
            },
        ];
        let indices = [0, 1, 2, 3];
        let mut monotone = [[true; 4]; 4];
        monotone[2][3] = false;
        assert!(!Guard::new(0, cards.clone(), &indices, &monotone, None).allows(0, 1));
        monotone[2][3] = true;
        let mut prior = [[true; 4]; 4];
        prior[2][3] = false;
        assert!(!Guard::new(0, cards.clone(), &indices, &monotone, Some(&prior)).allows(0, 1));
        prior[2][3] = true;
        // An unrelated band's lost uniform membership cannot veto this move.
        prior[3][3] = false;
        assert!(Guard::new(0, cards, &indices, &monotone, Some(&prior)).allows(0, 1));
    }

    #[test]
    fn accepted_changes_cover_every_possible_destination_team() {
        // Six characters, two attribute variants each. Exhaustively enumerate
        // four teammates and their cards independently of destination buckets.
        let identities: Vec<_> = (0..12)
            .map(|i| Identity {
                band: if i / 2 < 5 { 1 } else { 2 },
                attribute: (i % 2) as u8,
            })
            .collect();
        let context_of = |team: &[usize]| {
            usize::from(
                team.iter()
                    .all(|&i| identities[i].band == identities[team[0]].band),
            ) | (usize::from(
                team.iter()
                    .all(|&i| identities[i].attribute == identities[team[0]].attribute),
            ) << 1)
        };
        let mut checked = 0;
        for scenario in 0..8 {
            let mut prior = [[true; 4]; 12];
            let mut monotone = [[true; 4]; 12];
            if scenario & 1 != 0 {
                prior[6][3] = false;
            }
            if scenario & 2 != 0 {
                prior[8][2] = false;
            }
            if scenario & 4 != 0 {
                monotone[4][1] = false;
            }
            for context in 0..4 {
                let indices: Vec<_> = (0..12).filter(|&i| prior[i][context]).collect();
                let guard = Guard::new(
                    context,
                    identities.clone(),
                    &indices,
                    &monotone,
                    Some(&prior),
                );
                for &old in &indices {
                    for &new in &indices {
                        if !guard.allows(old, new) {
                            continue;
                        }
                        for a in 0..12 {
                            for b in a + 1..12 {
                                for c in b + 1..12 {
                                    for d in c + 1..12 {
                                        let teammates = [a, b, c, d];
                                        let roles: std::collections::BTreeSet<_> =
                                            teammates.iter().map(|i| i / 2).collect();
                                        if roles.len() != 4
                                            || roles.contains(&(old / 2))
                                            || roles.contains(&(new / 2))
                                            || teammates.iter().any(|&i| !prior[i][context])
                                        {
                                            continue;
                                        }
                                        if context_of(&[old, a, b, c, d]) != context {
                                            continue;
                                        }
                                        let team = [new, a, b, c, d];
                                        let destination = context_of(&team);
                                        assert_eq!(destination & context, context);
                                        assert!(team.iter().all(|&i| prior[i][destination]));
                                        if destination != context {
                                            assert!(team.iter().all(|&i| monotone[i][destination]));
                                        }
                                        checked += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1_000);
    }
}
