use super::*;

#[test]
fn suffixes_include_every_key_choice_but_do_not_invent_non_keys() {
    let rows = [vec![1, 2], vec![0, 4], vec![8], vec![0], vec![16, 32]];
    let suffix = suffix_masks(&rows);
    assert_eq!(suffix[5], vec![0]);
    assert_eq!(suffix[4], vec![16, 32]);
    let mut expected = vec![];
    for &a in &rows[0] {
        for &b in &rows[1] {
            for &c in &rows[2] {
                for &d in &rows[3] {
                    for &e in &rows[4] {
                        expected.push(a | b | c | d | e);
                    }
                }
            }
        }
    }
    expected.sort_unstable();
    expected.dedup();
    assert_eq!(suffix[0], expected);
    assert!(suffix[0].iter().all(|m| m & 8 != 0 && m & 3 != 0));
}

#[test]
fn coverage_requires_all_masks_and_all_songs_including_equal_bounds() {
    let mut table = vec![None; 8];
    table[1] = Some([10, 20, 30]);
    let mut checked = 0;
    assert!(!covered(&table, &[0, 2], 1, [10, 20, 30], &mut checked));
    table[3] = Some([10, 19, 30]);
    assert!(!covered(&table, &[0, 2], 1, [10, 20, 30], &mut checked));
    table[3] = Some([10, 20, 30]);
    assert!(covered(&table, &[0, 2], 1, [10, 20, 30], &mut checked));
    assert!(!covered(&table, &[4], 1, [0; 3], &mut checked));
}

fn queued_model() -> PoolTimingUpper {
    use bangdream_optimize_core::hit_timing::{
        independent::WindowOptions, NoteWindow, ReplayNote, ScoreEffect,
    };
    let notes: Vec<_> = (0..18)
        .map(|i| ReplayNote {
            time: (60 + i * 8) as f64 / 120.,
            trigger: (i % 3 == 0).then_some(i / 3),
            window: NoteWindow::Standard,
        })
        .collect();
    let skills: Vec<_> = (0..3)
        .map(|i| ReplaySkill {
            duration: 0.2 + i as f32 * 0.1,
            score: match i {
                0 => ScoreEffect::Plain { percent: 110. },
                1 => ScoreEffect::PerfectOnly { percent: 150. },
                _ => ScoreEffect::Crescendo {
                    percent: 100.,
                    step: 0.5,
                    cap: 150,
                },
            },
            correction: None,
        })
        .collect();
    let input = ReplayInput {
        power: 450000.5,
        play_level: 27,
        fps: 120,
        hit_frames: notes
            .iter()
            .map(|n| (n.time * 120.).round() as u32)
            .collect(),
        notes,
        skills: vec![skills[0].clone(); 6],
        initial_combo: 699,
        medley_combo: true,
        fever_frames: Some([75, 140]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    };
    PoolTimingUpper::new_adjacent_net(
        &input,
        &skills,
        ScoringPower::new(1_000_000.).unwrap(),
        WindowOptions::default(),
    )
    .unwrap()
}

#[test]
fn real_pool_bounds_preserve_all_occupancy_maxima_with_queued_skills() {
    let model = queued_model();
    type Options = [Vec<(usize, PoolTimingChoice)>; 5];
    fn visit(
        model: &PoolTimingUpper,
        opts: &Options,
        suffix: &[Vec<usize>; 6],
        values: &mut [PoolTimingChoice; 5],
        mask: usize,
        depth: usize,
        best: &mut [Option<[u64; 3]>],
        enabled: bool,
        skipped: &mut usize,
    ) {
        let cheap = model.separable_upper(values).unwrap();
        if enabled && covered(best, &suffix[depth], mask, [cheap; 3], &mut 0) {
            *skipped += 1;
            return;
        }
        if depth == 5 {
            let exact = model.upper(values).unwrap();
            assert!(exact <= cheap);
            let row = best[mask].get_or_insert([0; 3]);
            for s in 0..3 {
                row[s] = row[s].max(exact);
            }
        } else {
            let old = values[depth];
            for &(bit, choice) in &opts[depth] {
                values[depth] = choice;
                assert!(model.separable_upper(values).unwrap() <= cheap);
                visit(
                    model,
                    opts,
                    suffix,
                    values,
                    mask | bit,
                    depth + 1,
                    best,
                    enabled,
                    skipped,
                );
            }
            values[depth] = old;
        }
    }
    let mut results = vec![];
    for enabled in [false, true] {
        let mut best = vec![None; 32];
        let mut skipped = 0;
        for power in [90000.123, 50000.456, 10000.789] {
            let opts: Options = std::array::from_fn(|i| {
                vec![
                    (1 << i, model.choice(i % 3, power).unwrap()),
                    (0, model.choice((i + 1) % 3, power + 3.25).unwrap()),
                ]
            });
            let masks = suffix_masks(&opts.clone().map(|v| v.iter().map(|p| p.0).collect()));
            let mut values = opts.clone().map(|v| {
                v.into_iter()
                    .map(|p| p.1)
                    .reduce(PoolTimingChoice::union)
                    .unwrap()
            });
            visit(
                &model,
                &opts,
                &masks,
                &mut values,
                0,
                0,
                &mut best,
                enabled,
                &mut skipped,
            );
        }
        assert_eq!(best.iter().filter(|x| x.is_some()).count(), 32);
        if enabled {
            assert!(skipped > 0);
        }
        results.push(best);
    }
    assert_eq!(results[0], results[1]);
}

#[test]
fn profile_groups_cover_each_original_card_and_tighten_complete_mask_envelopes() {
    let model = queued_model();
    let make = |skill: usize, power: f64| -> Choice {
        std::array::from_fn(|s| model.choice((skill + s) % 3, power + s as f64).unwrap())
    };
    // Profile equivalence must include all songs, not just the first song.
    let mut special = make(0, 40_000.);
    special[2] = model.choice(1, 40_002.).unwrap();
    let actual = [
        vec![
            (1, make(0, 30_000.)),
            (0, make(1, 60_000.)),
            (0, make(1, 70_000.)),
            (0, make(2, 90_000.)),
        ],
        vec![(2, make(2, 60_000.)), (0, make(0, 40_000.)), (0, special)],
        vec![(4, make(2, 80_000.)), (0, make(0, 50_000.))],
        vec![(0, make(1, 60_000.))],
        vec![(0, make(2, 50_000.))],
    ];
    fn envelope(
        model: &PoolTimingUpper,
        options: &[Vec<(usize, Choice)>; 5],
    ) -> BTreeMap<usize, [u64; 3]> {
        fn visit(
            model: &PoolTimingUpper,
            options: &[Vec<(usize, Choice)>; 5],
            values: &mut [Choice; 5],
            depth: usize,
            mask: usize,
            result: &mut BTreeMap<usize, [u64; 3]>,
        ) {
            if depth == 5 {
                let upper: [u64; 3] =
                    std::array::from_fn(|s| model.upper(&values.map(|c| c[s])).unwrap());
                let row = result.entry(mask).or_insert([0; 3]);
                for s in 0..3 {
                    row[s] = row[s].max(upper[s]);
                }
            } else {
                for &(bit, choice) in &options[depth] {
                    values[depth] = choice;
                    visit(model, options, values, depth + 1, mask | bit, result);
                }
            }
        }
        let mut result = BTreeMap::new();
        visit(
            model,
            options,
            &mut [Choice::default(); 5],
            0,
            0,
            &mut result,
        );
        result
    }
    let grouped = |split| {
        std::array::from_fn(|g| {
            let mut result = Vec::new();
            let mut others = Vec::new();
            for &(mask, choice) in &actual[g] {
                if mask != 0 {
                    result.push((mask, choice));
                } else {
                    merge_non_key(&mut others, choice, split);
                }
            }
            result.extend(others.into_iter().map(|c| (0, c)));
            result
        })
    };
    let split = grouped(true);
    assert_eq!(split[0].len(), 3); // same triple merges, different duration stays separate
    assert_eq!(split[1].len(), 3); // the third song alone distinguishes these profiles
    let original = envelope(&model, &actual);
    let tightened = envelope(&model, &split);
    let loose = envelope(&model, &grouped(false));
    assert_eq!(
        original.keys().collect::<Vec<_>>(),
        tightened.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        original.keys().collect::<Vec<_>>(),
        loose.keys().collect::<Vec<_>>()
    );
    let mut strict = 0;
    for (&mask, old) in &original {
        for s in 0..3 {
            assert!(old[s] <= tightened[&mask][s]);
            assert!(tightened[&mask][s] <= loose[&mask][s]);
            strict += usize::from(tightened[&mask][s] < loose[&mask][s]);
        }
    }
    assert!(strict > 0);
    // Empty non-key domains cannot invent an unoccupied option.
    let mut empty = Vec::new();
    assert!(empty.is_empty());
    merge_non_key(&mut empty, make(0, 10_000.), true);
    assert_eq!(empty.len(), 1);
}
