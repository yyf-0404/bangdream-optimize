use super::*;

fn stateful() -> ReplayInput {
    let mut request = input(
        &[1., 1.05, 4.1, 10.],
        vec![
            skill(ScoreEffect::Crescendo {
                percent: 100.,
                step: 0.5,
                cap: 150,
            }),
            skill(ScoreEffect::Continuous {
                high: 120.,
                low: 80.,
            }),
        ],
    );
    request.skills[0].correction = Some(JudgementCorrection::Bad);
    request.initial_combo = 19;
    request.fever_frames = Some([123, 492]);
    request
}

#[test]
fn equivalent_power_scores_match_uncached_dp_across_integer_boundaries() {
    let request = stateful();
    let mut cache = TimingCompileCache::new(&request, SearchOptions::default());
    let bank = cache.prepare(&request.skills).unwrap();
    let mut cached = TimingScoreScratch {
        reuse_equivalent_scores: true,
        ..TimingScoreScratch::default()
    };
    let mut control = TimingScoreScratch {
        reuse_equivalent_scores: false,
        ..TimingScoreScratch::default()
    };
    let mut powers: Vec<_> = (0..32)
        .map(|offset| f32::from_bits(300_040.75_f32.to_bits() + offset))
        .collect();
    powers.extend([2., 2., 700_000.5, 700_000.5]);
    for p in powers {
        let power = ScoringPower::new(p).unwrap();
        assert_eq!(
            bank.score(power, &mut cached).unwrap(),
            bank.score(power, &mut control).unwrap(),
            "power={p}"
        );
    }
    assert!(cached.exact_stats.equivalent_score_hits > 2);
    assert!(cached.exact_stats.dp_queries > 2);
    assert!(cached.exact_stats.dp_queries < control.exact_stats.dp_queries);
    assert_eq!(
        cached.exact_stats.dp_queries + cached.exact_stats.equivalent_score_hits,
        control.exact_stats.dp_queries
    );
    let power = ScoringPower::new(700_000.5).unwrap();
    assert_eq!(
        bank.score(power, &mut cached).unwrap(),
        bank.materialize(power).unwrap().total
    );
}

#[test]
fn equal_power_keys_never_share_results_between_skill_or_chart_models() {
    let request = stateful();
    let mut cache = TimingCompileCache::new(&request, SearchOptions::default());
    let first = cache.prepare(&request.skills).unwrap();
    let mut changed = request.clone();
    changed.skills[1] = skill(ScoreEffect::Plain { percent: 200. });
    let second = cache.prepare(&changed.skills).unwrap();
    changed.initial_combo = 400;
    changed.fever_frames = None;
    let mut other_cache = TimingCompileCache::new(&changed, SearchOptions::default());
    let third = other_cache.prepare(&changed.skills).unwrap();
    let mut scratch = TimingScoreScratch::default();
    scratch.reuse_equivalent_scores = true;
    let power = ScoringPower::new(request.power).unwrap();
    for bank in [&first, &second, &third, &first] {
        assert_eq!(
            bank.score(power, &mut scratch).unwrap(),
            bank.exact.as_ref().unwrap().score(power).unwrap()
        );
    }
    assert_eq!(scratch.exact_stats.dp_queries, 4);
    assert_eq!(scratch.exact_stats.equivalent_score_hits, 0);
}

#[test]
fn uncacheable_power_preserves_the_original_scoring_error() {
    let request = stateful();
    let mut cache = TimingCompileCache::new(&request, SearchOptions::default());
    let bank = cache.prepare(&request.skills).unwrap();
    let mut scratch = TimingScoreScratch::default();
    let power = ScoringPower::new(f32::MAX).unwrap();
    assert!(bank.exact.as_ref().unwrap().score_signature(power).is_none());
    let actual = bank.score(power, &mut scratch).unwrap_err();
    let expected = bank.exact.as_ref().unwrap().score(power).unwrap_err();
    assert_eq!(actual.to_string(), expected.to_string());
    assert_eq!(scratch.exact_stats.equivalent_score_hits, 0);
}
