use super::*;

fn enumerate(members: [usize; 5], mut visit: impl FnMut([usize; 6])) {
    fn orders(
        members: [usize; 5],
        used: u8,
        order: &mut [usize; 6],
        visit: &mut impl FnMut([usize; 6]),
    ) {
        let slot = used.count_ones() as usize;
        if slot == 5 {
            for i in 0..5 {
                if i == 0 || members[i] != members[i - 1] {
                    order[5] = i;
                    visit(*order);
                }
            }
            return;
        }
        for i in 0..5 {
            if used & (1 << i) != 0
                || i > 0 && members[i] == members[i - 1] && used & (1 << (i - 1)) == 0
            {
                continue;
            }
            order[slot] = i;
            orders(members, used | (1 << i), order, visit);
        }
    }
    orders(members, 0, &mut [0; 6], &mut visit);
}

fn reference(
    domains: &[Vec<(u32, Judgement)>; 6],
    ranks: [usize; 6],
    members: [usize; 5],
    playing: [u32; 5],
    correction: [u32; 5],
    value: impl Fn(usize, usize, u32) -> f64,
) -> (f64, TimingAdjacentStats) {
    let mut answer = 0_f64;
    let mut stats = TimingAdjacentStats::default();
    enumerate(members, |order| {
        let upper = super::super::schedule(
            domains,
            ranks,
            order.map(|i| playing[i]),
            order.map(|i| correction[i]),
            WindowOptions::default(),
            &mut stats,
            |s, start, _| Ok(value(s, order[s], start)),
        )
        .unwrap()
        .expect("test oracle must complete");
        answer = answer.max(upper);
    });
    (answer, stats)
}

#[test]
fn joint_roster_matches_all_orders_with_duplicates_queues_and_corrections() {
    let base_playing = [61, 19, 31, 49, 13];
    let base_correction = [3, 0, 2, 0, 3];
    for members in [[0, 1, 2, 3, 4], [0, 0, 1, 2, 2], [0, 0, 0, 0, 0]] {
        let playing = members.map(|i| base_playing[i]);
        let correction = members.map(|i| base_correction[i]);
        for times in [
            [30, 40, 50, 200, 207, 500],
            [30, 30, 30, 100, 100, 500],
            [30, 200, 400, 700, 1000, 1300],
        ] {
            for ranks in [[0, 1, 2, 3, 4, 5], [1, 0, 2, 4, 3, 5]] {
                let domains = times.map(|t| {
                    vec![
                        (t + 2, Judgement::Bad),
                        (t, Judgement::Perfect),
                        (t + 1, Judgement::Good),
                    ]
                });
                let value = |s: usize, i: usize, frame: u32| {
                    f64::from((s as u32 * 17 + members[i] as u32 * 37 + frame * 13) % 157) / 7.
                };
                let (expected, _) = reference(&domains, ranks, members, playing, correction, value);
                let mut stats = TimingAdjacentStats::default();
                let actual = schedule(
                    &domains,
                    ranks,
                    members,
                    playing,
                    correction,
                    WindowOptions::default(),
                    &mut stats,
                    |s, i, frame, _| Ok(value(s, i, frame)),
                )
                .unwrap()
                .unwrap();
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "members={members:?}, times={times:?}, ranks={ranks:?}"
                );
                assert_eq!(stats.roster_budget_fallbacks, 0);
                // This completed sequence is an incumbent for the maximum of
                // local upper bounds, never a lower bound on a playable score.
                let order = [0, 1, 2, 3, 4, 0];
                let seed = super::super::schedule(
                    &domains,
                    ranks,
                    order.map(|i| playing[i]),
                    order.map(|i| correction[i]),
                    WindowOptions::default(),
                    &mut TimingAdjacentStats::default(),
                    |s, frame, _| Ok(value(s, order[s], frame)),
                )
                .unwrap()
                .unwrap();
                let capped = schedule_bounded(
                    &domains,
                    ranks,
                    members,
                    playing,
                    correction,
                    [[157. / 7.; 6]; 5],
                    seed,
                    WindowOptions::default(),
                    &mut TimingAdjacentStats::default(),
                    |s, i, frame, _| Ok(value(s, i, frame)),
                )
                .unwrap()
                .unwrap();
                assert_eq!(capped.to_bits(), expected.to_bits());
            }
        }
    }
}

#[test]
fn joint_roster_preserves_each_possible_captain_and_shared_idle_prefixes() {
    let domains = std::array::from_fn(|s| {
        (0..3)
            .rev()
            .map(|f| (30 + s as u32 * 1000 + f, Judgement::Perfect))
            .collect::<Vec<_>>()
    });
    let members = [0, 1, 2, 3, 4];
    for captain in 0..5 {
        let value = |s: usize, i: usize, frame: u32| {
            if s == 5 {
                if i == captain {
                    1000.
                } else {
                    0.
                }
            } else {
                f64::from((s as u32 * 17 + i as u32 * 37 + frame * 13) % 157) / 7.
            }
        };
        let (expected, old_stats) = reference(
            &domains,
            [0, 1, 2, 3, 4, 5],
            members,
            [61; 5],
            [0; 5],
            value,
        );
        let mut stats = TimingAdjacentStats::default();
        let actual = schedule(
            &domains,
            [0, 1, 2, 3, 4, 5],
            members,
            [61; 5],
            [0; 5],
            WindowOptions::default(),
            &mut stats,
            |s, i, frame, _| Ok(value(s, i, frame)),
        )
        .unwrap()
        .unwrap();
        assert_eq!(actual.to_bits(), expected.to_bits());
        assert_eq!(stats.peak_roster_states, 10); // C(5, 2) == C(5, 3).
        assert_eq!(stats.roster_transitions, 85 * 3); // 80 subset edges + 5 captains.
        assert!(stats.merged_roster_prefixes > 0);
        assert!(stats.roster_transitions * 10 < old_stats.schedule_transitions);
    }
}

#[test]
fn joint_roster_keeps_cooling_boundary_and_older_correcting_intervals() {
    let members = [0, 1, 2, 3, 4];
    let playing = [61, 19, 31, 49, 13];
    let edge = 30 + playing[0] + countdown(0.75) + 2;
    let domains = [
        vec![(30, Judgement::Perfect)],
        vec![(40, Judgement::Perfect)],
        vec![(50, Judgement::Bad)],
        vec![
            (edge + 1, Judgement::Perfect),
            (edge - 1, Judgement::Perfect),
            (edge, Judgement::Great),
        ],
        vec![(500, Judgement::Perfect)],
        vec![(700, Judgement::Perfect)],
    ];
    let value = |s: usize, i: usize, frame: u32| {
        f64::from((s as u32 * 17 + i as u32 * 37 + frame * 13) % 157) / 7.
    };
    let correction = [3, 0, 0, 0, 0];
    let (expected, _) = reference(
        &domains,
        [0, 1, 2, 3, 4, 5],
        members,
        playing,
        correction,
        value,
    );
    assert!(expected > 0.);
    let mut stats = TimingAdjacentStats::default();
    let actual = schedule(
        &domains,
        [0, 1, 2, 3, 4, 5],
        members,
        playing,
        correction,
        WindowOptions::default(),
        &mut stats,
        |s, i, frame, _| Ok(value(s, i, frame)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(actual.to_bits(), expected.to_bits());
    assert!(stats.peak_roster_states > 1);
}

#[test]
fn joint_roster_budget_requires_a_whole_roster_fallback() {
    let domains = std::array::from_fn(|s| vec![(30 + s as u32 * 1000, Judgement::Perfect)]);
    for options in [
        WindowOptions {
            max_transitions: 1,
            ..Default::default()
        },
        WindowOptions {
            max_states: 1,
            ..Default::default()
        },
    ] {
        let mut stats = TimingAdjacentStats::default();
        let answer = schedule(
            &domains,
            [0, 1, 2, 3, 4, 5],
            [0, 1, 2, 3, 4],
            [61; 5],
            [0; 5],
            options,
            &mut stats,
            |_, _, _, _| Ok(10.),
        )
        .unwrap();
        assert!(answer.is_none());
        assert_eq!(stats.roster_budget_fallbacks, 1);
    }
    let caps: Vec<[f64; 6]> = (0..5)
        .map(|i| std::array::from_fn(|s| f64::from(((i + 1) * 17 + s * 37) as u32) / 7.))
        .collect();
    for members in [[0, 1, 2, 3, 4], [0, 0, 1, 2, 2], [0; 5]] {
        let mut expected = 0_f64;
        enumerate(members, |order| {
            let cap = order
                .iter()
                .enumerate()
                .fold(0_f64, |v, (s, &i)| (v + caps[members[i]][s]).next_up());
            expected = expected.max(cap);
        });
        assert_eq!(
            independent_cap(members, &caps).to_bits(),
            expected.to_bits()
        );
    }
}

#[test]
fn best_assignment_seed_and_remaining_cap_preserve_forward_rounding() {
    fn visit(mask: u8, score: f64, caps: &[[f64; 6]; 5], best: &mut f64) {
        if mask == 31 {
            for row in caps {
                *best = best.max((score + row[5]).next_up());
            }
            return;
        }
        let slot = mask.count_ones() as usize;
        for (i, row) in caps.iter().enumerate() {
            if mask & (1 << i) == 0 {
                visit(mask | (1 << i), (score + row[slot]).next_up(), caps, best);
            }
        }
    }
    let caps: [[f64; 6]; 5] = std::array::from_fn(|i| {
        std::array::from_fn(|s| {
            let sign = if (i + s) % 2 == 0 { 1. } else { -1. };
            sign * ((i * 17 + s * 37 + 1) as f64 / 7.)
        })
    });
    for prefix in [-1e18, -0.01, 0., 1e9] {
        for mask in 0..32 {
            let mut expected = f64::NEG_INFINITY;
            visit(mask, prefix, &caps, &mut expected);
            assert_eq!(
                remaining_cap(prefix, mask, &caps).to_bits(),
                expected.to_bits()
            );
        }
    }
    for caps in [caps, [[-0.1; 6]; 5]] {
        let (bound, order) = independent_best([0, 1, 2, 3, 4], &caps);
        let score = order
            .iter()
            .enumerate()
            .fold(0_f64, |v, (s, &i)| (v + caps[i][s]).next_up());
        let mut expected = f64::NEG_INFINITY;
        visit(0, 0., &caps, &mut expected);
        assert_eq!(score.to_bits(), expected.to_bits());
        assert_eq!(bound.to_bits(), expected.max(0.).to_bits());
    }
}

#[test]
fn evaluated_best_cap_seed_can_avoid_all_joint_window_queries() {
    let domains = std::array::from_fn(|s| vec![(30 + s as u32 * 1000, Judgement::Perfect)]);
    let caps: [[f64; 6]; 5] = std::array::from_fn(|i| {
        std::array::from_fn(|s| if i == s || s == 5 && i == 3 { 100. } else { 1. })
    });
    let members = [0, 1, 2, 3, 4];
    let (cap, order) = independent_best(members, &caps);
    let seed = super::super::schedule(
        &domains,
        [0, 1, 2, 3, 4, 5],
        [61; 6],
        [0; 6],
        WindowOptions::default(),
        &mut TimingAdjacentStats::default(),
        |s, _, _| Ok(caps[order[s]][s]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(cap.to_bits(), seed.to_bits());
    let mut stats = TimingAdjacentStats::default();
    let mut calls = 0;
    let answer = schedule_bounded(
        &domains,
        [0, 1, 2, 3, 4, 5],
        members,
        [61; 5],
        [0; 5],
        caps,
        seed,
        WindowOptions::default(),
        &mut stats,
        |s, i, _, _| {
            calls += 1;
            Ok(caps[i][s])
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(answer.to_bits(), seed.to_bits());
    assert_eq!(calls, 0);
    assert!(stats.roster_bound_pruned > 0);
    assert_eq!(stats.roster_budget_fallbacks, 0);
}
