use super::*;
use crate::hit_timing::{JudgementCorrection, ReplayNote, ReplaySkill};

fn input(score: ScoreEffect, correction: Option<JudgementCorrection>) -> ReplayInput {
    ReplayInput {
        power: 466061.125,
        play_level: 27,
        fps: 120,
        notes: vec![
            ReplayNote {
                time: 0.2,
                trigger: Some(0),
                window: NoteWindow::Standard,
            },
            ReplayNote {
                time: 0.3,
                trigger: Some(1),
                window: NoteWindow::WidenedEndpoint,
            },
            ReplayNote {
                time: 0.32,
                trigger: None,
                window: NoteWindow::PinnedPerfect { frame: 38 },
            },
            ReplayNote {
                time: 0.6,
                trigger: None,
                window: NoteWindow::PinnedPerfect { frame: 72 },
            },
        ],
        hit_frames: vec![24, 36, 38, 72],
        skills: vec![
            ReplaySkill {
                duration: 0.1,
                score,
                correction
            };
            2
        ],
        initial_combo: 699,
        medley_combo: true,
        fever_frames: Some([30, 50]),
        constraints: vec![],
        hit_order: vec![],
        native_time_routing: None,
    }
}

#[test]
fn range_upper_encloses_exhaustive_replays_and_tightens_fixed_original_frames() {
    let mut strict = false;
    for effect in [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::PerfectOrHalf { percent: 135. },
        ScoreEffect::Continuous { high: 150., low: 80. },
        ScoreEffect::Crescendo { percent: 100., step: 5., cap: 150 },
        ScoreEffect::Crescendo { percent: 130., step: 0.5, cap: 130 },
    ] {
        for correction in [None,Some(JudgementCorrection::Bad)] {
            let r=input(effect,correction);
            let unlimited=TimingUpperCache::new(&r).prepare(&r.skills).unwrap();
            for (a,b) in [(0,0),(2,3)] {
                let limits=vec![Some([24-a,24+a]),Some([36-b,36+b]),None,None];
                let upper=TimingUpperCache::new(&r).with_frame_ranges(limits.clone())
                    .prepare(&r.skills).unwrap();
                for value in [0.125,300040.75,700000.5] {
                    let power=ScoringPower::new(value).unwrap();
                    let bounded=upper.score(power,&mut TimingUpperScratch::default()).unwrap();
                    let loose=unlimited.score(power,&mut TimingUpperScratch::default()).unwrap();
                    assert!(bounded<=loose);
                    strict|=bounded<loose;
                    let mut q=r.clone();q.power=value;
                    let mut valid=0;
                    for x in 24-a..=24+a { for y in 36-b..=36+b {
                        q.hit_frames[0]=x;q.hit_frames[1]=y;
                        if let Ok(result)=replay(&q) {
                            valid+=1;
                            assert!(result.total<=bounded,"{effect:?} {correction:?} {x} {y}");
                        }
                    }}
                    assert!(valid>0);
                }
            }
        }
    }
    assert!(strict);
    let r=input(ScoreEffect::Plain {percent:100.},None);
    for limits in [vec![],vec![Some([1,0]);4],vec![Some([0,0]);4]] {
        assert!(TimingUpperCache::new(&r).with_frame_ranges(limits).prepare(&r.skills).is_err());
    }
}

#[test]
fn upper_encloses_every_legal_trigger_pair_with_queue_correction_fever_and_combo() {
    for effect in [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::PerfectOrHalf { percent: 135. },
        ScoreEffect::Continuous {
            high: 150.,
            low: 80.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 5.,
            cap: 150,
        },
        // Equal base/cap can make the first native increment slightly negative.
        ScoreEffect::Crescendo {
            percent: 130.,
            step: 0.5,
            cap: 130,
        },
    ] {
        let r = input(effect, Some(JudgementCorrection::Bad));
        let bound = TimingScoreUpper::new(&r).unwrap();
        let domains = search::frame_domains(&r).unwrap();
        let envelope = bound
            .upper_envelope(ScoringPower::new(1_000_000.).unwrap())
            .unwrap();
        for value in [0.125, 300040.75, 700000.5] {
            let p = ScoringPower::new(value).unwrap();
            let upper = bound.score(p, &mut TimingUpperScratch::default()).unwrap();
            assert!(upper <= envelope.upper(p).unwrap());
            let mut q = r.clone();
            q.power = value;
            let mut valid = 0;
            for &a in &domains[0] {
                for &b in &domains[1] {
                    q.hit_frames[0] = a;
                    q.hit_frames[1] = b;
                    if let Ok(result) = replay(&q) {
                        valid += 1;
                        assert!(
                            result.total <= upper,
                            "{effect:?}, frames {a},{b}: {} > {upper}",
                            result.total
                        );
                    }
                }
            }
            assert!(valid > 100);
        }
    }
}

#[test]
fn ordinary_note_frames_are_bounded_and_can_exceed_the_fixed_timeline() {
    let mut r = input(ScoreEffect::Plain { percent: 100. }, None);
    r.skills.truncate(1);
    r.notes.truncate(2);
    r.hit_frames.truncate(2);
    r.notes[1].trigger = None;
    r.notes[1].time = 0.36;
    r.hit_frames[1] = 43;
    r.initial_combo = 19;
    r.fever_frames = Some([30, 44]);
    let baseline = replay(&r).unwrap();
    let bound = TimingScoreUpper::new(&r).unwrap();
    let upper = bound
        .score(
            ScoringPower::new(r.power).unwrap(),
            &mut TimingUpperScratch::default(),
        )
        .unwrap();
    let domains = search::frame_domains(&r).unwrap();
    let mut best = baseline.total;
    for &a in &domains[0] {
        for &b in &domains[1] {
            let mut q = r.clone();
            q.hit_frames = vec![a, b];
            if let Ok(result) = replay(&q) {
                best = best.max(result.total);
                assert!(result.total <= upper);
            }
        }
    }
    assert!(best > baseline.total);
}

#[test]
fn chaining_three_windows_and_native_binding_are_conservatively_enclosed() {
    let mut r = input(
        ScoreEffect::Continuous {
            high: 130.,
            low: 50.,
        },
        Some(JudgementCorrection::Bad),
    );
    r.notes[2].trigger = Some(2);
    r.skills.push(r.skills[0].clone());
    let cases = [r, crate::hit_timing::time_connections::request()];
    for r in cases {
        let bound = TimingScoreUpper::new(&r).unwrap();
        for power in [1., 300040.75, 700000.5] {
            let mut q = r.clone();
            q.power = power;
            let found = search::optimize(&q, search::SearchOptions::default()).unwrap();
            assert!(
                found.result.total
                    <= bound
                        .score(
                            ScoringPower::new(power).unwrap(),
                            &mut TimingUpperScratch::default()
                        )
                        .unwrap()
            );
        }
    }
}

#[test]
fn unused_skill_slots_do_not_delay_real_requests_in_the_bound() {
    let mut r = input(ScoreEffect::Plain { percent: 150. }, None);
    r.notes[0].trigger = None;
    r.skills[0].duration = 60.;
    r.skills[1].duration = 1.;
    let bound = TimingScoreUpper::new(&r).unwrap();
    let result = replay(&r).unwrap();
    assert!(
        result.total
            <= bound
                .score(
                    ScoringPower::new(r.power).unwrap(),
                    &mut TimingUpperScratch::default()
                )
                .unwrap()
    );
    r.notes[1].trigger = None;
    let no_requests = TimingScoreUpper::new(&r).unwrap();
    assert!(
        replay(&r).unwrap().total
            <= no_requests
                .score(
                    ScoringPower::new(r.power).unwrap(),
                    &mut TimingUpperScratch::default()
                )
                .unwrap()
    );
}

#[test]
fn cached_geometry_and_queue_profiles_match_uncached_for_all_effects() {
    let effects = [
        ScoreEffect::Plain { percent: 100. },
        ScoreEffect::PerfectOnly { percent: 150. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::PerfectOrHalf { percent: 135. },
        ScoreEffect::Continuous {
            high: 150.,
            low: 80.,
        },
        ScoreEffect::Crescendo {
            percent: 100.,
            step: 5.,
            cap: 150,
        },
        ScoreEffect::Crescendo {
            percent: 130.,
            step: 0.5,
            cap: 130,
        },
    ];
    let mut overlap = input(effects[0], None);
    overlap.notes[2].window = NoteWindow::NativeGreen { first_frames: [1; 4], last_frame: 300 };
    let mut separated = input(effects[0], None);
    separated.notes.clear();
    separated.hit_frames.clear();
    separated.skills = vec![separated.skills[0].clone(); 6];
    for slot in 0..6 {
        for offset in [0., 0.05, 0.15, 0.25, 7.45, 7.55] {
            let time = 0.2 + 10.5 * slot as f64 + offset;
            separated.notes.push(ReplayNote { time, trigger: (offset == 0.).then_some(slot), window: NoteWindow::Standard });
            separated.hit_frames.push((time * 120.).round() as u32);
        }
    }
    for mut r in [
        input(effects[0], None),
        crate::hit_timing::time_connections::request(),
        overlap,
        separated,
    ] {
        if r.skills.is_empty() {
            r.skills.push(ReplaySkill {
                duration: 0.1,
                score: effects[0],
                correction: None,
            });
            r.notes[0].trigger = Some(0);
        }
        let mut cache = TimingUpperCache::new(&r);
        for correction in [
            None,
            Some(JudgementCorrection::Great),
            Some(JudgementCorrection::Good),
            Some(JudgementCorrection::Bad),
        ] {
            for effect in effects {
                for duration in [0.1, 0.2, 7.5] {
                    r.skills[0] = ReplaySkill {
                        duration,
                        score: effect,
                        correction,
                    };
                    let direct = TimingScoreUpper::new(&r).unwrap();
                    let prior = cache.prepare_impl(&r.skills, false).unwrap();
                    let cached = cache.prepare_impl(&r.skills, true).unwrap();
                    // Compare complete operand alternatives, not merely a few
                    // powers whose rounding might hide a wrong cache hit.
                    let canonical = |upper: &TimingScoreUpper| {
                        upper
                            .choices
                            .iter()
                            .map(|(ids, count)| {
                                let mut terms: Vec<_> =
                                    ids.iter().map(|&i| upper.terms[i]).collect();
                                terms.sort_unstable();
                                (terms, *count)
                            })
                            .collect::<BTreeMap<_, _>>()
                    };
                    assert_eq!(canonical(&direct), canonical(&cached));
                    assert_eq!(canonical(&prior), canonical(&cached));
                    for value in [0.125, 300040.75, 700000.5] {
                        let p = ScoringPower::new(value).unwrap();
                        let mut scratch = TimingUpperScratch::default();
                        assert_eq!(
                            direct.score(p, &mut scratch).unwrap(),
                            cached.score(p, &mut scratch).unwrap()
                        );
                        assert_eq!(
                            direct.upper_envelope(p).unwrap().upper(p).unwrap(),
                            cached.upper_envelope(p).unwrap().upper(p).unwrap()
                        );
                        assert_eq!(prior.upper_envelope(p).unwrap().upper(p).unwrap(),
                            cached.upper_envelope(p).unwrap().upper(p).unwrap());
                    }
                }
            }
        }
        let built = cache.stats().profile_builds;
        cache.prepare(&r.skills).unwrap();
        assert_eq!(built, cache.stats().profile_builds);
        assert!(cache.stats().profile_hits > 0);
        assert!(cache.stats().disjoint_preparations > 0);
        r.skills[0].duration = 0.;
        assert!(cache.prepare(&r.skills).is_err());
    }
}
