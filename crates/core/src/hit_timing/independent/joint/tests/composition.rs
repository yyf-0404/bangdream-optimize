//! The oracle enumerates frames through replay, not any bound or replacement
//! table. These tests check the contracts at the joint-scoring boundary.
use super::*;
use crate::hit_timing::bounds::{TimingAffineContext, TimingNetUpper, TimingReplacementBounds};
use crate::scoring::{DeckPower, MemberPower};

fn tiny(chain: bool) -> ReplayInput {
    let times = if chain {
        [0.1, 0.15, 0.2, 0.25, 0.3, 0.35, 4.5]
    } else {
        [0.1, 1.4, 2.7, 4., 5.3, 6.6, 6.75]
    };
    let mut r = input(&times, vec![profiles()[0].clone(); 6], 1);
    // The last note spans a Perfect/Great boundary while Fever also changes.
    r.notes[6].time -= 0.05;
    r.fever_frames = Some([r.hit_frames[6], r.hit_frames[6] + 1]);
    r
}

#[test]
fn all_delayed_upper_modes_and_budget_fallback_cover_full_frame_oracles() {
    let mut skills = profiles();
    skills[1].duration = 0.8;
    for chain in [false, true] {
        let mut r = tiny(chain);
        let cap = ScoringPower::new(700000.5).unwrap();
        let limited = WindowOptions {
            max_states: 1,
            max_graph_edges: 1,
            max_transitions: 1,
            optimize_touch_order: false, // Pruning constructors must override this.
            ..WindowOptions::default()
        };
        let bounds = [
            TimingNetUpper::new(&r, &skills, cap).unwrap(),
            TimingNetUpper::new_stateful(&r, &skills, cap).unwrap(),
            TimingNetUpper::new_frontier_net(&r, &skills, cap, WindowOptions::default()).unwrap(),
            TimingNetUpper::new_adjacent_net(&r, &skills, cap, WindowOptions::default()).unwrap(),
            TimingNetUpper::new_frontier_net(&r, &skills, cap, limited).unwrap(),
            TimingNetUpper::new_adjacent_net(&r, &skills, cap, limited).unwrap(),
        ];
        for ids in [[0, 1, 2, 3, 4, 0], [5, 4, 3, 2, 1, 5], [8, 7, 6, 5, 4, 8]] {
            let sequence = ids.map(|i| skills[i].clone());
            r.skills = sequence.to_vec();
            let exact = JointTimeline::new(&r).unwrap();
            for p in [2., 300040.75, 700000.5] {
                r.power = p;
                let power = ScoringPower::new(p).unwrap();
                let oracle = brute(&r);
                assert_eq!(exact.score(power).unwrap(), oracle);
                for (mode, bound) in bounds.iter().enumerate() {
                    assert!(
                        bound.upper_sequence(&sequence, power).unwrap() >= oracle,
                        "mode={mode}, chain={chain}, sequence={ids:?}, power={p}"
                    );
                }
            }
        }
    }
}

fn member(value: u32) -> MemberPower {
    MemberPower::from_bonuses([value; 3], [0.; 3], [0.123; 3], 0., [0.; 3]).unwrap()
}

#[test]
fn accepted_replacement_certificates_preserve_complete_joint_optima_in_all_ten_roles() {
    use crate::hit_timing::ScoreEffect;
    let mut catalogue = profiles();
    catalogue.push(ReplaySkill {
        duration: 0.8,
        ..catalogue[0].clone()
    });
    catalogue.push(ReplaySkill {
        score: ScoreEffect::Plain { percent: 300. },
        ..catalogue[0].clone()
    });
    let old_member = member(1000);
    let new_member = member(18000);
    let other = member(4000);
    let range = [other.linear_total() * 4.; 2];
    let cap = ScoringPower::new(700000.5).unwrap();
    let mut accepted = [0; 5];
    for chain in [false, true] {
        let base = tiny(chain);
        let mut bounds = TimingReplacementBounds::new_adjacent(&base, &catalogue, cap).unwrap();
        bounds.tighten_frames(&base, &catalogue).unwrap();
        let ids: Vec<_> = (0..catalogue.len()).collect();
        bounds.set_replanned_catalogue(&ids);
        let coefficient = bounds.coefficient_upper(&ids);
        // All four teammates use catalogue[4]. Enclose all ten possible roles,
        // including whichever teammate supplies the encore.
        let rows = bounds.scenario_coefficients(4);
        let lo = rows.iter().map(|r| r[0]).fold(0., f64::min) * 4.;
        let hi = rows.iter().map(|r| r[1]).fold(0., f64::max) * 4.;
        let affine = TimingAffineContext {
            other_power: range,
            other_coefficient: [[lo, hi]; 10],
            required_score: 0.,
        };
        // Ordinary same-input, stateful, changed correction, changed duration.
        for (a, b) in [(10, 0), (5, 4), (0, 6), (8, 7), (9, 0), (0, 9)] {
            let passes = [
                bounds
                    .can_replace_affine_context(a, b, new_member, old_member, &affine, true)
                    .unwrap(),
                bounds
                    .can_replace_in_context(a, b, new_member, old_member, range, coefficient, 0.)
                    .unwrap(),
                bounds
                    .can_replace_extended_context(
                        a,
                        b,
                        new_member,
                        old_member,
                        range,
                        coefficient,
                        0.,
                    )
                    .unwrap(),
                bounds
                    .can_replace_correction_context(
                        a,
                        b,
                        new_member,
                        old_member,
                        range,
                        coefficient,
                        0.,
                    )
                    .unwrap(),
                bounds
                    .can_replace_replanned_context(
                        a,
                        b,
                        new_member,
                        old_member,
                        range,
                        coefficient,
                        0.,
                    )
                    .unwrap(),
            ];
            for (n, pass) in accepted.iter_mut().zip(passes) {
                *n += usize::from(pass);
            }
            if !passes.iter().any(|&p| p) {
                continue;
            }
            for slot in 0..5 {
                for captain in [false, true] {
                    let mut old = base.clone();
                    // Preserve all other skills; replacing the captain also
                    // replaces the encore, never just the first occurrence.
                    old.skills = vec![catalogue[4].clone(); 6];
                    old.skills[slot] = catalogue[b].clone();
                    if captain {
                        old.skills[5] = catalogue[b].clone();
                    }
                    let mut new = old.clone();
                    new.skills[slot] = catalogue[a].clone();
                    if captain {
                        new.skills[5] = catalogue[a].clone();
                    }
                    let mut members = [other; 5];
                    members[slot] = old_member;
                    old.power = DeckPower::from_members(&members).unwrap().scoring().value();
                    members[slot] = new_member;
                    new.power = DeckPower::from_members(&members).unwrap().scoring().value();
                    let before = brute(&old);
                    let after = brute(&new);
                    assert!(after >= before,
                        "{a}->{b}, slot={slot}, captain={captain}, chain={chain}, certificates={passes:?}");
                    assert_eq!(
                        JointTimeline::new(&new)
                            .unwrap()
                            .score(ScoringPower::new(new.power).unwrap())
                            .unwrap(),
                        after
                    );
                }
            }
        }
    }
    assert!(
        accepted.iter().all(|&n| n > 0),
        "each proof must actually accept a replacement: {accepted:?}"
    );
}
