use super::*;
use crate::{
    hit_timing::{JudgementCorrection, ReplayNote, ReplayResult, SkillEvent},
    scoring::MemberPower,
};

#[test]
fn proof_row_cells_keep_every_start_bit_and_reduce_actual_rows() {
    for combo in [0,699] {
        let (mut input,skills)=sample();
        input.initial_combo=combo; input.fever_frames=Some([70,145]);
        let mut original=build(&input,&skills); original.row_cells_enabled=false;
        original.runs=None;
        let mut compressed=build(&input,&skills); compressed.row_cells_enabled=true;
        let (mut expected,mut actual)=(Stats::default(),Stats::default());
        for slot in 0..6 { for id in 0..skills.len() { for start in 0..360 {
            assert_eq!(compressed.old_upper(slot,id,start,&mut actual).to_bits(),
                original.old_upper(slot,id,start,&mut expected).to_bits(),
                "old combo={combo}, slot={slot}, id={id}, start={start}");
        } } }
        for plan in 0..compressed.plans.len() { for id in 0..skills.len() { for start in 0..360 {
            assert_eq!(compressed.new_lower(plan,id,start,&mut actual).to_bits(),
                original.new_lower(plan,id,start,&mut expected).to_bits(),
                "new combo={combo}, plan={plan}, id={id}, start={start}");
        } } }
        assert!(actual.old_rows<expected.old_rows);
        assert!(actual.new_rows<expected.new_rows);
        assert!(actual.old_cell_aliases>0 && actual.new_cell_aliases>0);
        assert!(compressed.row_cells.as_ref().unwrap().bytes()<=cells::MAX_BYTES);
        eprintln!("proof cells combo={combo}: old rows {} -> {}, new rows {} -> {}, frame evaluations {} -> {}",
            expected.old_rows,actual.old_rows,expected.new_rows,actual.new_rows,
            expected.old_frame_evaluations,actual.old_frame_evaluations);
    }
}

#[test]
fn sparse_shuffled_cell_boundaries_keep_count_changes_and_capacity_fallbacks() {
    let (input,skills)=sample();
    let mut original=build(&input,&skills); original.row_cells_enabled=false;
    original.runs=None; original.indexed=false;
    let notes:Vec<_>=original.notes.iter().map(|fs| {
        let mut fs:Vec<_>=fs.iter().copied().filter(|f| fs.len()==1 || f.time%7!=0).collect();
        fs.reverse(); fs
    }).collect();
    original.notes=Arc::new(notes);
    original.spans=original.notes.iter().map(|fs| (
        fs.iter().map(|f| f.time).min().unwrap(),fs.iter().map(|f| f.time).max().unwrap())).collect();
    original.first=std::array::from_fn(|slot| {
        let mut times:Vec<_>=original.spans.iter().enumerate()
            .filter(|&(i,_)| original.triggers[i].is_none_or(|s| s>slot))
            .map(|(_,span)| span.0).collect(); times.sort_unstable();times
    });
    original.last=std::array::from_fn(|slot| {
        let mut times:Vec<_>=original.spans.iter().enumerate()
            .filter(|&(i,_)| original.triggers[i].is_none_or(|s| s>slot))
            .map(|(_,span)| span.1).collect(); times.sort_unstable();times
    });
    let index=Arc::new(cells::Index::new(&original.profiles,&original.notes));
    let mut compressed=build(&input,&skills); compressed.row_cells_enabled=true;
    compressed.notes=original.notes.clone(); compressed.runs=None; compressed.indexed=false;
    compressed.spans=original.spans.clone(); compressed.first=original.first.clone();
    compressed.last=original.last.clone(); compressed.row_cells=Some(index);
    let (mut expected,mut actual)=(Stats::default(),Stats::default());
    for slot in 0..6 { for id in 0..skills.len() { for start in 0..360 {
        assert_eq!(compressed.old_upper(slot,id,start,&mut actual).to_bits(),
            original.old_upper(slot,id,start,&mut expected).to_bits());
    } } }
    let profiles=vec![original.profiles[0].clone();20000];
    let empty=cells::Index::new(&profiles,&original.notes);
    assert!(empty.bytes()<=cells::MAX_BYTES);
    assert_eq!(empty.start(0,profiles[0].playing,100),100);
    assert_eq!(empty.start(0,profiles[0].playing,u32::MAX),u32::MAX);
}

#[test]
fn shared_primitive_rows_and_max_runs_keep_every_bit_and_work_boundary() {
    for combo in [0,699] {
        let (mut input,skills)=sample(); input.initial_combo=combo;
        let owner=TimingReplacementCache::new(&input,&skills);
        for power in [300040.75,600000.,800000.,300040.75] {
            let mut prepared=owner.prepare_tightened(ScoringPower::new(power).unwrap()).unwrap();
            let shared=prepared.replanned.as_mut().unwrap();
            let mut reference=build(&input,&skills); reference.runs=None; reference.row_cells_enabled=false;
            let (mut stats,mut expected_stats)=(Stats::default(),Stats::default());
            for slot in 0..6 { for id in 0..skills.len() { for start in (35..260).step_by(7) {
                assert_eq!(shared.old_upper(slot,id,start,&mut stats).to_bits(),
                    reference.old_upper(slot,id,start,&mut expected_stats).to_bits());
            } } }
            for plan in 0..shared.plans.len() { for id in 0..skills.len() { for start in (35..260).step_by(7) {
                assert_eq!(shared.new_lower(plan,id,start,&mut stats).to_bits(),
                    reference.new_lower(plan,id,start,&mut expected_stats).to_bits());
            } } }
            for limit in [0,1,25,1000,WORK_LIMIT] {
                let mut prepared=owner.prepare_tightened(ScoringPower::new(power).unwrap()).unwrap();
                let shared=prepared.replanned.as_mut().unwrap();
                shared.shared=None; shared.work_limit=limit;
                let mut reference=build(&input,&skills); reference.runs=None; reference.row_cells_enabled=false; reference.work_limit=limit;
                let (mut stats,mut expected_stats)=(Stats::default(),Stats::default());
                for ids in [vec![0,1],vec![0,2,4]] {
                    shared.set_catalogue(&ids); reference.set_catalogue(&ids);
                    for (a,b) in [(1,4),(4,1),(3,5)] {
                        assert_eq!(shared.difference(a,b,&mut stats).map(f64::to_bits),
                            reference.difference(a,b,&mut expected_stats).map(f64::to_bits));
                        assert_eq!(shared.work,reference.work);
                        assert_eq!(stats.transitions,expected_stats.transitions);
                        assert_eq!(stats.budget_fallbacks,expected_stats.budget_fallbacks);
                    }
                }
            }
        }
        assert!(owner.frame_cache_stats().row_hits>0);
        let reversed:Vec<_>=skills.into_iter().rev().collect();
        let other=TimingReplacementCache::new(&input,&reversed);
        assert_eq!(other.frame_cache_stats().row_hits,0);
        assert_eq!(other.frame_cache_stats().row_payload_bytes,0);
    }
}

fn sample() -> (ReplayInput, Vec<ReplaySkill>) {
    let skills = vec![
        ReplaySkill {
            duration: 0.10,
            score: ScoreEffect::Plain { percent: 10. },
            correction: None,
        },
        ReplaySkill {
            duration: 0.35,
            score: ScoreEffect::Plain { percent: 180. },
            correction: None,
        },
        ReplaySkill {
            duration: 0.20,
            score: ScoreEffect::Continuous {
                high: 160.,
                low: 20.,
            },
            correction: None,
        },
        ReplaySkill {
            duration: 0.30,
            score: ScoreEffect::Crescendo {
                percent: 100.,
                step: 0.5,
                cap: 150,
            },
            correction: None,
        },
        ReplaySkill {
            duration: 0.40,
            score: ScoreEffect::Plain { percent: 120. },
            correction: Some(JudgementCorrection::Bad),
        },
        ReplaySkill {
            duration: 0.30,
            score: ScoreEffect::PerfectOrZero { percent: 100. },
            correction: None,
        },
    ];
    let input = ReplayInput {
        power: 300_040.75,
        play_level: 27,
        fps: 120,
        notes: (0..12)
            .map(|i| ReplayNote {
                time: (60 + 15 * i) as f64 / 120.,
                trigger: (i % 2 == 0).then_some(i / 2),
                window: if [1, 3, 7].contains(&i) {
                    NoteWindow::Standard
                } else {
                    NoteWindow::PinnedPerfect {
                        frame: (60 + 15 * i) as u32,
                    }
                },
            })
            .collect(),
        hit_frames: (0..12).map(|i| 60 + 15 * i).collect(),
        skills: vec![skills[0].clone(); 6],
        initial_combo: 699,
        medley_combo: true,
        fever_frames: Some([78, 175]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    };
    (input, skills)
}

fn coefficient(input: &ReplayInput, replay: &ReplayResult) -> f64 {
    let scale =
        f64::from(1_f32 + (input.play_level - 5) as f32 * 0.01_f32) / input.notes.len() as f64 * 3.;
    replay
        .trace
        .iter()
        .map(|t| {
            scale
                * f64::from(t.judgement.multiplier())
                * f64::from(input.combo_multiplier(t.combo))
                * f64::from(t.fever_multiplier)
                * f64::from(t.skill_multiplier)
        })
        .sum()
}

fn build(input: &ReplayInput, skills: &[ReplaySkill]) -> Replanned {
    let frames = frames::FrameBounds::new(input, skills).unwrap();
    Replanned::new(input, skills, &frames).unwrap()
}

#[test]
fn witness_rows_match_full_native_clock_and_skill_histories() {
    let (input, skills) = sample();
    let mut model = build(&input, &skills);
    assert_eq!(model.plans.len(), 3);
    let mut stats = Stats::default();
    let mut shifted = 0;
    for p in 0..model.plans.len() {
        for bits in 0..32 {
            let ids: Vec<_> = (0..6)
                .map(|s| {
                    if bits & (1 << (s % 5)) != 0 {
                        s % skills.len()
                    } else {
                        4
                    }
                })
                .collect();
            let mut current = input.clone();
            current.hit_frames = model.plans[p].hit_frames.clone();
            current.skills = ids.iter().map(|&i| skills[i].clone()).collect();
            let result = replay(&current).unwrap();
            let mut actual_starts = [0; 6];
            for e in &result.skill_events {
                if let SkillEvent::Start {
                    index, after_frame, ..
                } = *e
                {
                    actual_starts[index] = after_frame;
                }
            }
            let mut ready = 0;
            let mut lower = model.plans[p].base;
            for s in 0..6 {
                let start = ready.max(model.plans[p].requests[s]);
                assert_eq!(start, actual_starts[s]);
                shifted += usize::from(start > model.plans[p].requests[s]);
                lower += model.new_lower(p, ids[s], start, &mut stats);
                ready = start + model.profiles[ids[s]].playing + countdown(0.75) + 2;
            }
            let actual = coefficient(&current, &result);
            assert!(
                lower <= actual + 1e-12 && actual - lower < 1e-10,
                "{lower} versus {actual}"
            );
        }
    }
    assert!(shifted > 0);
}

#[test]
fn different_plan_difference_covers_all_roles_and_sampled_old_inputs() {
    let (input, skills) = sample();
    let mut model = build(&input, &skills);
    let mut stats = Stats::default();
    let mut checked = 0;
    let mut raw_bad = 0;
    for (a, b) in [(1, 4), (0, 1), (3, 5), (4, 0)] {
        let delta = model.difference(a, b, &mut stats).unwrap();
        for role in 0..10 {
            for bits in 0..8 {
                let mut ids: Vec<_> = (0..6)
                    .map(|s| if bits & (1 << (s % 3)) != 0 { 2 } else { 4 })
                    .collect();
                ids[role % 5] = b;
                if role >= 5 {
                    ids[5] = b;
                }
                let mut old = input.clone();
                old.skills = ids.iter().map(|&i| skills[i].clone()).collect();
                let mut new = old.clone();
                new.skills[role % 5] = skills[a].clone();
                if role >= 5 {
                    new.skills[5] = skills[a].clone();
                }
                let new_best = model
                    .plans
                    .iter()
                    .map(|p| {
                        new.hit_frames = p.hit_frames.clone();
                        coefficient(&new, &replay(&new).unwrap())
                    })
                    .fold(f64::NEG_INFINITY, f64::max);
                for offset in [-6, 0, 6, 15] {
                    old.hit_frames[1] = (input.hit_frames[1] as i32 + offset) as u32;
                    let Ok(result) = replay(&old) else { continue };
                    raw_bad += result
                        .trace
                        .iter()
                        .filter(|t| t.raw < Judgement::Great)
                        .count();
                    assert!(delta <= new_best - coefficient(&old, &result) + 1e-11);
                    // The upper envelope is independently checked per complete
                    // old replay, including skill keys covered by earlier skills.
                    let mut upper = model.reference_total;
                    for e in &result.skill_events {
                        if let SkillEvent::Start {
                            index, after_frame, ..
                        } = *e
                        {
                            upper += model.old_upper(index, ids[index], after_frame, &mut stats);
                        }
                    }
                    assert!(coefficient(&old, &result) <= upper + 1e-11);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 500 && raw_bad > 0);
    assert_eq!(stats.budget_fallbacks, 0);
}

#[test]
fn certificate_can_remove_bad_correction_and_change_duration_and_inputs() {
    let (mut input, skills) = sample();
    input.skills = vec![skills[4].clone(); 6];
    input.hit_frames[1] += 15;
    let old = replay(&input).unwrap();
    assert!(old.trace.iter().any(|t| t.raw < Judgement::Great));
    let mut same_inputs = input.clone();
    same_inputs.skills[0] = skills[1].clone();
    assert!(replay(&same_inputs).is_err());
    let mut bound = TimingReplacementBounds::new_adjacent(
        &input,
        &skills,
        ScoringPower::new(1_000_000.).unwrap(),
    )
    .unwrap();
    bound.tighten_frames(&input, &skills).unwrap();
    let power = |axis| MemberPower::from_bonuses([axis; 3], [0.; 3], [0.; 3], 0., [0.; 3]).unwrap();
    assert!(bound
        .can_replace_replanned_context(
            1,
            4,
            power(120_000),
            power(10_000),
            [120_000.; 2],
            bound.coefficient_upper(&(0..skills.len()).collect::<Vec<_>>()),
            0.
        )
        .unwrap());
    let model = bound.replanned.as_ref().unwrap();
    assert!(model.plans.iter().all(|p| p.hit_frames != input.hit_frames));
    for p in &model.plans {
        same_inputs.hit_frames = p.hit_frames.clone();
        assert!(replay(&same_inputs).is_ok());
    }
    // Exercise the published native-score certificate, including the alternate
    // coefficient decomposition and physical f32 member accumulation.
    for role in 0..10 {
        for layout in crate::hit_timing::layouts::physical_layouts()
            .iter()
            .step_by(23)
        {
            let mut old = input.clone();
            let mut new = old.clone();
            new.skills[role % 5] = skills[1].clone();
            if role >= 5 {
                new.skills[5] = skills[1].clone();
            }
            let before = [power(10_000); 5];
            let mut after = before;
            after[0] = power(120_000);
            old.power =
                ScoringPower::from_member_parameters(&layout.map(|i| before[i].final_parameters()))
                    .unwrap()
                    .value();
            new.power =
                ScoringPower::from_member_parameters(&layout.map(|i| after[i].final_parameters()))
                    .unwrap()
                    .value();
            let old_score = replay(&old).unwrap().total;
            let new_score = model
                .plans
                .iter()
                .map(|p| {
                    new.hit_frames = p.hit_frames.clone();
                    replay(&new).unwrap().total
                })
                .max()
                .unwrap();
            assert!(new_score >= old_score);
        }
    }
}

#[test]
fn catalogue_change_invalidates_suffixes_and_budget_keeps_candidate() {
    let (input, skills) = sample();
    let mut model = build(&input, &skills);
    let mut stats = Stats::default();
    model.set_catalogue(&[0]);
    let narrow = model.difference(1, 4, &mut stats).unwrap();
    assert_eq!(Some(narrow), model.difference(1, 4, &mut stats));
    assert!(stats.hits > 0);
    model.set_catalogue(&(0..skills.len()).collect::<Vec<_>>());
    let broad = model.difference(1, 4, &mut stats).unwrap();
    assert!(broad <= narrow + 1e-12);
    let mut fresh = build(&input, &skills);
    fresh.work_limit = 0;
    assert!(fresh.difference(1, 4, &mut stats).is_none());
    assert!(stats.budget_fallbacks > 0);
}

#[test]
fn continuation_rows_share_storage_and_survive_catalogue_invalidation() {
    let (input, skills) = sample();
    let mut model = build(&input, &skills);
    let mut stats = Stats::default();
    model.set_catalogue(&[0]);
    let key = (0, 2, State { old: 200, new: 220 });
    let first = model.common_edges(key, &mut stats).unwrap();
    let before: Vec<_> = first.iter().map(|e| (e.next, e.delta.to_bits())).collect();
    let cached = model.common_edges(key, &mut stats).unwrap();
    assert!(Arc::ptr_eq(&first, &cached));
    let prefix = model.prefix(0, &mut stats).unwrap();
    assert!(Arc::ptr_eq(&prefix, &model.prefix(0, &mut stats).unwrap()));

    // Outstanding readers own their rows, but a changed successor catalogue
    // must not obtain those rows through the now-invalid cache.
    model.set_catalogue(&(0..skills.len()).collect::<Vec<_>>());
    let rebuilt = model.common_edges(key, &mut stats).unwrap();
    assert!(!Arc::ptr_eq(&first, &rebuilt));
    assert!(!Arc::ptr_eq(&prefix, &model.prefix(0, &mut stats).unwrap()));
    assert_eq!(
        before,
        first
            .iter()
            .map(|e| (e.next, e.delta.to_bits()))
            .collect::<Vec<_>>()
    );
    let mut fresh = build(&input, &skills);
    let expected = fresh.common_edges(key, &mut Stats::default()).unwrap();
    assert_eq!(
        rebuilt
            .iter()
            .map(|e| (e.next, e.delta.to_bits()))
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|e| (e.next, e.delta.to_bits()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn paired_edge_view_keeps_rounding_and_whole_row_budget_boundary() {
    let (mut input, skills) = sample();
    input.notes[2].window = NoteWindow::Standard;
    let mut model = build(&input, &skills);
    let mut stats = Stats::default();
    let slot = 1;
    let state = State { old: 0, new: 180 };
    let old = model.old_edges(slot, 4, state.old, &mut stats);
    assert!(old.len() > 1);
    let new_start = state.new.max(model.plans[0].requests[slot]);
    let gain = model.new_lower(0, 1, new_start, &mut stats);
    let next_new = model
        .canonical(
            0,
            slot + 1,
            State {
                old: 0,
                new: new_start + model.profiles[1].playing + countdown(0.75) + 2,
            },
        )
        .new;
    let expected: Vec<_> = old
        .iter()
        .map(|&(ready, upper)| {
            (
                State {
                    old: ready,
                    new: next_new,
                },
                down(gain - upper).to_bits(),
            )
        })
        .collect();
    for limit in [0, 1, old.len() - 1, old.len(), old.len() + 1] {
        model.work = 0;
        model.work_limit = limit;
        let before = stats.transitions;
        let edges = model.pair_edges(0, slot, state, 1, 4, &mut stats);
        assert_eq!(model.work, old.len().min(limit + 1));
        assert_eq!(stats.transitions - before, model.work);
        if limit < old.len() {
            assert!(edges.is_none());
        } else {
            let edges = edges.unwrap();
            assert!(Arc::ptr_eq(&old, &edges.old));
            assert_eq!(
                expected,
                edges
                    .iter()
                    .map(|e| (e.next, e.delta.to_bits()))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn indexed_continuations_match_hash_maps_at_cache_and_work_limits() {
    let (mut input, skills) = sample();
    input.notes[2].window = NoteWindow::Standard;
    input.notes[4].window = NoteWindow::Standard;
    // Zero/small limits exercise uncached rows, mixed indexed/overflow rows,
    // and budget failure after some successful cache admissions.
    for row_limit in [0, 3, CACHE_LIMIT] {
        for work_limit in [0, 1, 25, 1_000, WORK_LIMIT] {
            let mut reference = build(&input, &skills);
            reference.continuations = cache::Cache::unindexed(row_limit);
            reference.work_limit = work_limit;
            let mut reference_stats = Stats::default();
            let mut expected = Vec::new();
            for (a, b) in [(1, 4), (4, 1), (3, 5), (1, 4), (0, 1)] {
                let result = reference.difference(a, b, &mut reference_stats);
                expected.push((
                    result.map(f64::to_bits),
                    reference.work,
                    serde_json::to_value(&reference_stats).unwrap(),
                ));
            }
            for index_limit in [1, 7, CACHE_LIMIT] {
                let mut indexed = build(&input, &skills);
                indexed.continuations = cache::Cache::small(index_limit, row_limit);
                indexed.work_limit = work_limit;
                let mut stats = Stats::default();
                for ((a, b), expected) in [(1, 4), (4, 1), (3, 5), (1, 4), (0, 1)]
                    .into_iter()
                    .zip(&expected)
                {
                    let result = indexed.difference(a, b, &mut stats);
                    assert_eq!(
                        (result.map(f64::to_bits), indexed.work, serde_json::to_value(&stats).unwrap()),
                        *expected,
                        "row limit {row_limit}, index limit {index_limit}, work {work_limit}, pair {a}/{b}"
                    );
                }
            }
        }
    }
}

#[test]
fn losing_witness_pruning_keeps_exact_differences_across_domains_and_chains() {
    let (mut input, skills) = sample();
    input.notes[2].window = NoteWindow::Standard;
    input.notes[4].window = NoteWindow::Standard;
    let chain = crate::hit_timing::chain_tests::fixture();
    let chain_skills = chain.skills[..5].to_vec();
    let mut prunes = 0;
    let mut saved_work = 0;
    for (input, skills) in [(input, skills), (chain, chain_skills)] {
        let mut reference = build(&input, &skills);
        let mut pruned = build(&input, &skills);
        reference.work_limit = usize::MAX;
        pruned.work_limit = usize::MAX;
        let mut reference_stats = Stats::default();
        let mut stats = Stats::default();
        // Include both directions, correction changes, captain encore and a
        // narrow->wide transition. All ten roles and all witnesses are visited.
        for ids in [vec![0, 1], (0..skills.len()).collect(), vec![0, 2, 4]] {
            reference.set_catalogue(&ids);
            pruned.set_catalogue(&ids);
            for a in 0..skills.len() {
                for b in 0..skills.len() {
                    let expected = reference
                        .compute_with_witness_pruning::<false>(a, b, &mut reference_stats)
                        .unwrap();
                    let actual = pruned.compute(a, b, &mut stats).unwrap();
                    assert_eq!(actual.to_bits(), expected.to_bits(), "{ids:?}: {a}/{b}");
                }
            }
        }
        prunes += stats.witness_prunes;
        saved_work += reference_stats.transitions - stats.transitions;
    }
    assert!(prunes > 0 && saved_work > 0);
}

#[test]
fn rejected_suffix_is_not_cached_as_a_complete_minimum() {
    let (input, skills) = sample();
    let mut model = build(&input, &skills);
    let mut stats = Stats::default();
    let key = (0, 3, State { old: 0, new: 0 });
    let index = model.continuations.index(key);
    let mut captain = model.continuations.begin_captain();
    assert!(matches!(
        model.captain_probe(
            key,
            index,
            (1, 4),
            &mut captain,
            WitnessCutoff::new(f64::INFINITY, 0., 0.),
            &mut stats,
        ),
        Some(Probe::Rejected)
    ));
    assert!(model.continuations.captain(key, index, &captain).is_none());
    let Some(Probe::Complete(actual)) = model.captain_probe(
        key,
        index,
        (1, 4),
        &mut captain,
        WitnessCutoff::new(f64::NEG_INFINITY, 0., 0.),
        &mut stats,
    ) else {
        panic!("an unrestricted probe must complete");
    };
    let mut fresh = build(&input, &skills);
    let index = fresh.continuations.index(key);
    let mut captain = fresh.continuations.begin_captain();
    let expected = fresh
        .suffix(
            key,
            index,
            Some((1, 4)),
            &mut captain,
            &mut Stats::default(),
        )
        .unwrap();
    assert_eq!(actual.to_bits(), expected.to_bits());
    assert!(stats.suffix_prunes > 0);
}

#[test]
fn exhaustive_two_movable_notes_include_a_request_and_lost_correction() {
    let (mut input, skills) = sample();
    for (i, n) in input.notes.iter_mut().enumerate() {
        n.window = if [1, 2].contains(&i) {
            NoteWindow::Standard
        } else {
            NoteWindow::PinnedPerfect {
                frame: input.hit_frames[i],
            }
        };
    }
    let mut model = build(&input, &skills);
    model.set_catalogue(&[0, 4]);
    let mut stats = Stats::default();
    let delta = model.difference(1, 4, &mut stats).unwrap();
    let domains: Vec<_> = model
        .notes
        .iter()
        .map(|fs| fs.iter().map(|f| f.time).collect::<Vec<_>>())
        .collect();
    let mut checked = 0;
    let mut different_requests = 0;
    for role in [0, 1, 5, 6] {
        for bits in 0..4 {
            let mut old = input.clone();
            old.skills = (0..6)
                .map(|s| skills[if bits & (1 << (s % 2)) == 0 { 0 } else { 4 }].clone())
                .collect();
            old.skills[role % 5] = skills[4].clone();
            if role >= 5 {
                old.skills[5] = skills[4].clone();
            }
            let mut new = old.clone();
            new.skills[role % 5] = skills[1].clone();
            if role >= 5 {
                new.skills[5] = skills[1].clone();
            }
            let best = model
                .plans
                .iter()
                .map(|p| {
                    new.hit_frames = p.hit_frames.clone();
                    coefficient(&new, &replay(&new).unwrap())
                })
                .fold(f64::NEG_INFINITY, f64::max);
            for &a in &domains[1] {
                for &b in &domains[2] {
                    old.hit_frames[1] = a;
                    old.hit_frames[2] = b;
                    let Ok(result) = replay(&old) else { continue };
                    assert!(delta <= best - coefficient(&old, &result) + 1e-11);
                    checked += 1;
                    different_requests += usize::from(b != input.hit_frames[2]);
                }
            }
        }
    }
    assert!(checked > 1_000 && different_requests > 500);
    assert_eq!(stats.budget_fallbacks, 0);
}

#[test]
fn reverse_maxima_and_queued_edges_match_uncompressed_request_enumeration() {
    let (mut input, skills) = sample();
    input.notes[2].window = NoteWindow::Standard;
    input.notes[4].window = NoteWindow::Standard;
    let mut model = build(&input, &skills);
    let mut stats = Stats::default();
    for slot in 0..6 {
        for id in 0..skills.len() {
            for ready in (0..350).step_by(7) {
                let mut expected = BTreeMap::<u32, f64>::new();
                for request in model.requests[slot].clone() {
                    let start = ready.max(request);
                    let next = start + model.profiles[id].playing + countdown(0.75) + 2;
                    let next = if slot == 5 || next <= model.requests[slot + 1][0] {
                        0
                    } else {
                        next
                    };
                    let upper = model.old_upper(slot, id, start, &mut stats);
                    expected
                        .entry(next)
                        .and_modify(|v| *v = v.max(upper))
                        .or_insert(upper);
                }
                let actual = model.old_edges(slot, id, ready, &mut stats);
                assert_eq!(&*actual, &expected.into_iter().collect::<Vec<_>>());
                assert_eq!(&*actual, &*model.old_edges(slot, id, ready, &mut stats));
            }
        }
    }
    assert!(stats.suffix_rows > 0 && stats.old_edge_hits > 0);
}

#[test]
fn replacement_covers_scored_chain_through_encore_and_both_duration_directions() {
    use crate::hit_timing::chain_tests::{fixture, starts};
    let input = fixture();
    let skills = input.skills[..5].to_vec();
    let mut model = build(&input, &skills);
    let mut stats = Stats::default();
    let mut checked = 0;
    let mut propagated = 0;
    let mut encore_scored = 0;
    for (a, b) in [(0, 1), (1, 0), (2, 0)] {
        let delta = model.difference(a, b, &mut stats).unwrap();
        assert_eq!(Some(delta), model.difference(a, b, &mut stats));
        for role in 0..10 {
            let mut old = input.clone();
            old.skills[role % 5] = skills[b].clone();
            if role >= 5 {
                old.skills[5] = skills[b].clone();
            }
            let mut new = old.clone();
            new.skills[role % 5] = skills[a].clone();
            if role >= 5 {
                new.skills[5] = skills[a].clone();
            }
            let new_best = model
                .plans
                .iter()
                .map(|p| {
                    new.hit_frames = p.hit_frames.clone();
                    let result = replay(&new).unwrap();
                    let start_frames = starts(&result);
                    let mut ready = 0;
                    let mut lower = p.base;
                    // Independently sum each complete witness's actual windows;
                    // later chain slots must contribute, not merely exist in the clock.
                    for s in 0..6 {
                        let id = if s == role % 5 || role >= 5 && s == 5 {
                            a
                        } else {
                            s % 5
                        };
                        let start = ready.max(p.requests[s]);
                        assert_eq!(start, start_frames[s]);
                        let profile = &model.profiles[id];
                        ready = start + profile.playing + countdown(0.75) + 2;
                        let first = p.notes.partition_point(|&(t, _)| t <= start);
                        let last = p
                            .notes
                            .partition_point(|&(t, _)| t <= start + profile.playing);
                        for (count, &(_, weight)) in p.notes[first..last].iter().enumerate() {
                            lower += weight * (f64::from(profile.states[count][1]) - 1.);
                        }
                    }
                    let actual = coefficient(&new, &result);
                    assert!((actual - lower).abs() < 1e-10);
                    actual
                })
                .fold(f64::NEG_INFINITY, f64::max);
            // Vary an already queued request and all selected late-note frames.
            // Invalid corrected inputs are rejected by the native replay.
            for offset in [-4, 0, 4, 12] {
                for ordinary_offset in [-4, 0, 4, 9] {
                    old.hit_frames = input.hit_frames.clone();
                    old.hit_frames[2] = (old.hit_frames[2] as i32 + offset) as u32;
                    for i in 6..old.notes.len() {
                        if matches!(old.notes[i].window, NoteWindow::Standard) {
                            old.hit_frames[i] = (old.hit_frames[i] as i32 + ordinary_offset) as u32;
                        }
                    }
                    let Ok(result) = replay(&old) else { continue };
                    assert!(
                        delta <= new_best - coefficient(&old, &result) + 1e-11,
                        "pair {a}/{b}, role {role}, frames {offset}/{ordinary_offset}"
                    );
                    encore_scored +=
                        usize::from(result.trace.iter().any(|n| n.active_skill == Some(5)));
                    new.hit_frames = input.hit_frames.clone();
                    let new_starts = starts(&replay(&new).unwrap());
                    let old_starts = starts(&result);
                    if role % 5 == 0 && (1..6).all(|s| new_starts[s] != old_starts[s]) {
                        propagated += 1;
                    }
                    checked += 1;
                }
            }
        }
    }
    assert!(checked >= 300 && propagated > 0 && encore_scored == checked);
    assert_eq!(stats.budget_fallbacks, 0);
}

#[test]
fn equipment_cache_shares_only_completed_differences_in_the_same_successor_domain() {
    let (input, skills) = sample();
    let cache = TimingReplacementCache::new(&input, &skills);
    let domains = [vec![0], vec![0, 4], (0..skills.len()).collect::<Vec<_>>()];
    let pairs = [(1, 4), (0, 1), (4, 2)];
    let mut expected = Vec::new();
    for domain in &domains {
        let mut fresh = build(&input, &skills);
        fresh.set_catalogue(domain);
        expected.push(pairs.map(|(a, b)| fresh.difference(a, b, &mut Stats::default())));
    }
    for (iteration, power) in [400000., 700000.25].into_iter().enumerate() {
        let cap = ScoringPower::new(power).unwrap();
        let mut outer = cache.prepare(cap).unwrap();
        outer.tighten_frames(&input, &skills).unwrap();
        let mut fresh = TimingReplacementBounds::new_adjacent(&input, &skills, cap).unwrap();
        fresh.tighten_frames(&input, &skills).unwrap();
        assert_eq!(outer.score_error, fresh.score_error);
        assert_eq!(outer.power_error, fresh.power_error);
        let model = outer.replanned.as_mut().unwrap();
        let mut stats = Stats::default();
        for (domain, row) in domains.iter().zip(&expected) {
            model.set_catalogue(domain);
            assert_eq!(pairs.map(|(a, b)| model.difference(a, b, &mut stats)), *row);
        }
        if iteration == 1 {
            assert_eq!(stats.shared_hits, 9);
            assert_eq!(stats.transitions, 0);
        }
    }
    // A rejected budget is not published to other equipment models.
    let mut outer = cache.prepare(ScoringPower::new(500000.).unwrap()).unwrap();
    outer.tighten_frames(&input, &skills).unwrap();
    let model = outer.replanned.as_mut().unwrap();
    model.set_catalogue(&[1, 3]);
    model.work_limit = 0;
    let mut stats = Stats::default();
    let size = cache.shared.lock().unwrap().len();
    assert!(model.difference(1, 4, &mut stats).is_none());
    assert_eq!(cache.shared.lock().unwrap().len(), size);
    assert_eq!(stats.budget_fallbacks, 1);
}
