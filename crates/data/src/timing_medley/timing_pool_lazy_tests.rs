use super::lazy_competitive;

#[test]
fn refined_roster_audit_uses_the_same_catalogue_rounding_envelope() {
    use bangdream_optimize_core::{
        hit_timing::{bounds::{PoolTimingUpper, TimingNetUpper},
            replay, NoteWindow, ReplayInput, ReplayNote, ReplaySkill, ScoreEffect},
        scoring::ScoringPower,
    };
    let weak = ReplaySkill {
        duration: 0.1, score: ScoreEffect::Plain { percent: 5. }, correction: None,
    };
    let removed = ReplaySkill {
        duration: 10., score: ScoreEffect::Plain { percent: 20_000. }, correction: None,
    };
    let skills = vec![weak.clone(), removed];
    let notes: Vec<_> = (0..6).map(|slot| {
        let frame = 120 * (slot as u32 + 1);
        ReplayNote { time: f64::from(frame) / 120., trigger: Some(slot),
            window: NoteWindow::PinnedPerfect { frame } }
    }).collect();
    let input = ReplayInput {
        power: 300_040.75, play_level: 27, fps: 120,
        hit_frames: (1..=6).map(|i| i * 120).collect(), notes,
        skills: vec![weak.clone(); 6], initial_combo: 0, medley_combo: true,
        fever_frames: None, constraints: vec![], hit_order: vec![], native_time_routing: None,
    };
    let cap = ScoringPower::new(600_000.).unwrap();
    let power = ScoringPower::new(input.power).unwrap();
    let members = std::array::from_fn(|_| weak.clone());
    let cases = [[0, 1, 0, 1]; 5];
    let allowed = [[true, false, true, false]; 5];
    let surviving = super::audit_catalogue(&skills, &cases, &allowed, true);
    let pool = PoolTimingUpper::new_adjacent_net(&input, &surviving, cap, Default::default()).unwrap();
    let choices = std::array::from_fn(|_| pool.choice(0, f64::from(power) / 5.).unwrap());
    let bound = pool.upper(&choices).unwrap();
    let old = TimingNetUpper::new_adjacent_net(&input, &skills, cap, Default::default()).unwrap();
    let same = TimingNetUpper::new_adjacent_net(&input, &surviving, cap, Default::default()).unwrap();
    assert!(old.upper_roster(&members, power).unwrap() > bound,
        "a wider source envelope is not a lower bound on the refined upper");
    let audited = same.upper_roster(&members, power).unwrap();
    assert!(replay(&input).unwrap().total <= audited);
    assert!(audited <= bound);
    assert_eq!(super::audit_catalogue(&skills, &cases, &allowed, false), skills);
    assert_eq!(super::audit_catalogue(&skills, &cases, &[[false; 4]; 5], true), skills);
}

#[test]
fn deferred_songs_match_eager_boolean_for_missing_negative_and_equal_thresholds() {
    let triple = |n: usize| [n % 4, (n / 4) % 4, n / 16].map(|v| v as u64);
    let contexts = [
        [Some(0), Some(0), Some(0)],
        [Some(-2), Some(3), None],
        [None; 3],
    ];
    for c in 0..64 {
        let cheap = triple(c);
        for f in 0..64 {
            let fine = triple(f);
            if (0..3).any(|s| fine[s] > cheap[s]) {
                continue;
            }
            for other in contexts {
                for seed in 0..8 {
                    let mut calls = [0; 3];
                    let actual = lazy_competitive(cheap, other, seed, |s| {
                        calls[s] += 1;
                        fine[s]
                    });
                    let expected = (0..3).any(|s| {
                        other[s].is_some_and(|o| i128::from(fine[s]) + o >= i128::from(seed))
                    });
                    assert_eq!(actual, expected);
                    assert!(calls.into_iter().all(|n| n <= 1));
                }
            }
        }
    }
}

#[test]
fn surviving_first_song_avoids_other_refinements_without_using_cheap_as_score() {
    let mut calls = vec![];
    assert!(lazy_competitive([100; 3], [Some(0); 3], 90, |s| {
        calls.push(s);
        90
    }));
    assert_eq!(calls, vec![0]);
    calls.clear();
    assert!(!lazy_competitive(
        [89, 100, 100],
        [Some(0), None, Some(0)],
        90,
        |s| {
            calls.push(s);
            89
        }
    ));
    assert_eq!(calls, vec![2]);
    assert!(lazy_competitive(
        [u64::MAX; 3],
        [Some(i128::from(u64::MAX)); 3],
        u64::MAX,
        |_| 0
    ));
}
