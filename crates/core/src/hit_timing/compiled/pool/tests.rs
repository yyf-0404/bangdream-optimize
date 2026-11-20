use super::*;
use crate::hit_timing::{NoteWindow, ReplayNote, ScoreEffect};

fn input() -> ReplayInput {
    let times = [1.,1.05,4.1,10.];
    ReplayInput { power: 300040.75, play_level: 27, fps: 120,
        notes: times.into_iter().enumerate().map(|(i,time)| ReplayNote {
            time, trigger: (i == 0).then_some(0), window: NoteWindow::Standard,
        }).collect(), hit_frames: times.into_iter().map(|t| (t*120_f64).round() as u32).collect(),
        skills: vec![ReplaySkill {duration:3.,correction:None,score:ScoreEffect::Plain {percent:100.}}],
        initial_combo:699, medley_combo:true, fever_frames:Some([120,150]),
        constraints:vec![],hit_order:vec![],native_time_routing:None }
}
fn key(skills: &[ReplaySkill]) -> Vec<[u32;6]> { skills.iter().map(ReplaySkill::cache_key).collect() }

#[test]
fn shared_budget_evicts_oldest_across_charts_and_held_banks_keep_exact_witnesses() {
    let request = input();
    let mut pool = TimingCompileCachePool::with_geometry_budget(&vec![request.clone();3],
        SearchOptions::default(),usize::MAX);
    let first = pool.prepare(0,&request.skills).unwrap();
    let bytes = pool.caches[0].exact_geometry_bytes();
    let second = pool.prepare(1,&request.skills).unwrap();
    assert_eq!(pool.retained_bytes(),bytes*2);
    // Refresh chart 0 after chart 1, using the same pool clock.
    let refreshed = pool.prepare(0,&request.skills).unwrap();
    pool.max_geometry_bytes = bytes*2;
    pool.stats.budget_bytes = Some(bytes*2);
    let third = pool.prepare(2,&request.skills).unwrap();
    let skills_key = key(&request.skills);
    assert!(pool.caches[0].exact.contains_key(&skills_key));
    assert!(pool.caches[1].exact.is_empty());
    assert!(pool.caches[2].exact.contains_key(&skills_key));
    assert_eq!(pool.retained_bytes(),bytes*2);
    assert_eq!(pool.stats.evicted_bindings,1);
    assert_eq!(pool.stats.released_geometry_bytes,bytes as u64);
    for p in [0.125,request.power,f32::from_bits(request.power.to_bits()+1)] {
        let power = ScoringPower::new(p).unwrap();
        let expected = JointTimeline::new(&request).unwrap().maximum(power).unwrap().result;
        for bank in [&first,&second,&refreshed,&third] {
            assert_eq!(bank.score(power,&mut TimingScoreScratch::default()).unwrap(),expected.total);
            assert_eq!(serde_json::to_value(bank.materialize(power).unwrap()).unwrap(),
                serde_json::to_value(&expected).unwrap());
        }
    }
    let builds = pool.caches[1].stats.group_builds;
    pool.prepare(1,&request.skills).unwrap();
    assert_eq!(pool.caches[1].stats.group_builds,builds+1);
    assert!(!pool.caches[0].exact.contains_key(&skills_key));
    assert!(pool.retained_bytes() <= pool.max_geometry_bytes);
}

#[test]
fn shared_budget_counts_one_geometry_per_chart_and_evicts_all_sharing_bindings() {
    let request = input();
    let mut pool = TimingCompileCachePool::with_geometry_budget(&[request.clone()],
        SearchOptions::default(),usize::MAX);
    let first = pool.prepare(0,&request.skills).unwrap();
    let bytes = pool.retained_bytes();
    let mut skills = request.skills.clone();
    skills[0].score = ScoreEffect::PerfectOnly {percent:155.};
    let second = pool.prepare(0,&skills).unwrap();
    assert_eq!(pool.caches[0].exact.len(),2);
    assert_eq!(pool.retained_bytes(),bytes);
    assert_eq!(first.exact.as_ref().unwrap().geometry_identity(),second.exact.as_ref().unwrap().geometry_identity());
    pool.max_geometry_bytes = 0;
    pool.enforce();
    assert!(pool.caches[0].exact.is_empty());
    assert_eq!(pool.stats.evicted_bindings,2);
    assert_eq!(pool.stats.released_geometry_bytes,bytes as u64);
    assert_eq!(pool.retained_bytes(),0);
    let power = ScoringPower::new(request.power).unwrap();
    let mut direct = request.clone(); direct.skills = skills;
    assert_eq!(second.score(power,&mut TimingScoreScratch::default()).unwrap(),
        JointTimeline::new(&direct).unwrap().score(power).unwrap());
}

#[test]
fn zero_shared_budget_returns_complete_models_and_keeps_chart_preparation_reusable() {
    let request = input();
    let mut pool = TimingCompileCachePool::with_geometry_budget(&[request.clone()],
        SearchOptions::default(),0);
    let mut held = vec![];
    for duration in [3.,1.,4.,1.] {
        let mut direct = request.clone(); direct.skills[0].duration = duration;
        let bank = pool.prepare(0,&direct.skills).unwrap();
        let power = ScoringPower::new(request.power).unwrap();
        let expected = JointTimeline::new(&direct).unwrap().maximum(power).unwrap().result;
        assert_eq!(bank.score(power,&mut TimingScoreScratch::default()).unwrap(),expected.total);
        assert_eq!(serde_json::to_value(bank.materialize(power).unwrap()).unwrap(),serde_json::to_value(expected).unwrap());
        held.push(bank);
        assert_eq!(pool.retained_bytes(),0);
        assert_eq!(pool.stats.peak_retained_geometry_bytes,0);
    }
    assert_eq!(pool.caches[0].preparation_stats().chart_preparations,1);
    assert_eq!(pool.caches[0].preparation_stats().geometry_builds,4);
    assert_eq!(pool.stats.preparations,4);
    assert!(pool.stats.peak_pending_geometry_bytes > 0);
    let power = ScoringPower::new(request.power).unwrap();
    assert_eq!(held[0].materialize(power).unwrap().total,
        JointTimeline::new(&request).unwrap().maximum(power).unwrap().result.total);
}
