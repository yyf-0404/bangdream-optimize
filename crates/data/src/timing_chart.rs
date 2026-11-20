//! Chart preparation shared by native/WASM callers and probes.
//! The source adapter must bind native factory records to these exact score-note
//! identities. Preparation is independent of cards, skills, power and equipment.
pub mod time_distance;
use bangdream_optimize_core::hit_timing::{
    green::{
        launch::NativeBatch,
        motion::GreenGeometry,
        speed::AllSpeedChain,
        trajectory::{
            install_with_feasible_baseline, prepare_chart_standard_speeds, NativeGreenChain,
        },
    },
    ReplayError, ReplayInput,
};
use serde::{Deserialize, Serialize};

pub const NATIVE_GREEN_PREPARATION_REVISION: &str = "native-green-standard-1101-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeGreenSource {
    pub basic_bpm: f32,
    pub batches: Vec<NativeBatch>,
    pub chains: Vec<NativeGreenChain>,
    pub geometry: GreenGeometry,
}

/// Persist compact output, not 1101 trajectories. The original factory source,
/// geometry and exact scorer identities form the cache key. Skill/power changes
/// reuse this object but still obtain a freshly checked feasible baseline.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeGreenChartCache {
    revision: String,
    source: NativeGreenSource,
    note_identities: Vec<(u64, Option<usize>)>,
    prepared: Vec<AllSpeedChain>,
}
impl NativeGreenChartCache {
    pub fn prepare(source: NativeGreenSource, input: &ReplayInput) -> Result<Self, ReplayError> {
        if input.fps != 120 {
            return Err(ReplayError::Input(
                "native green preparation requires 120 FPS",
            ));
        }
        let prepared = prepare_chart_standard_speeds(
            &source.chains,
            source.basic_bpm,
            &source.batches,
            source.geometry,
        )?;
        let result = Self {
            revision: NATIVE_GREEN_PREPARATION_REVISION.into(),
            source,
            note_identities: identities(input),
            prepared,
        };
        // An incomplete/misbound chart may never enter the reusable cache.
        let mut check = input.clone();
        result.install(&mut check)?;
        Ok(result)
    }
    pub fn matches(&self, source: &NativeGreenSource, input: &ReplayInput) -> bool {
        self.revision == NATIVE_GREEN_PREPARATION_REVISION
            && input.fps == 120
            && self.note_identities == identities(input)
            && serde_json::to_value(&self.source)
                .ok()
                .zip(serde_json::to_value(source).ok())
                .is_some_and(|(a, b)| a == b)
    }
    pub fn install(&self, input: &mut ReplayInput) -> Result<(), ReplayError> {
        if self.revision != NATIVE_GREEN_PREPARATION_REVISION
            || input.fps != 120
            || identities(input) != self.note_identities
            || self.prepared.len() != self.source.chains.len()
        {
            return Err(ReplayError::Input(
                "native green preparation cache is stale",
            ));
        }
        for (source, prepared) in self.source.chains.iter().zip(&self.prepared) {
            if !source.nodes.iter().filter_map(|n| n.note_index).eq(prepared
                .compiled
                .windows
                .iter()
                .map(|n| n.0))
            {
                return Err(ReplayError::Input(
                    "native green cache node identities differ",
                ));
            }
        }
        install_with_feasible_baseline(&self.prepared, input)
    }
    pub fn standard_speed_count(&self) -> usize {
        1101
    }
    pub fn representative_settings(&self) -> Vec<u16> {
        self.prepared
            .iter()
            .flat_map(|c| c.representative_settings.iter().copied())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn green_note_count(&self) -> usize {
        self.prepared.iter().map(|c| c.compiled.windows.len()).sum()
    }
}
fn identities(input: &ReplayInput) -> Vec<(u64, Option<usize>)> {
    input
        .notes
        .iter()
        .map(|n| (n.time.to_bits(), n.trigger))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bangdream_optimize_core::hit_timing::{
        green::{launch::NativePosition, trajectory::NativeGreenNode, GreenRole},
        replay, NoteWindow, ReplayNote,
    };
    fn fixture() -> (NativeGreenSource, ReplayInput) {
        let positions = [384, 432, 480];
        let source = NativeGreenSource {
            basic_bpm: 120.,
            batches: positions
                .iter()
                .map(|&t| NativeBatch {
                    position: NativePosition {
                        bar: t / 192,
                        numerator: t % 192,
                        denominator: 192,
                    },
                    empty: false,
                    bpm: None,
                })
                .collect(),
            chains: vec![NativeGreenChain {
                root_batch: 0,
                nodes: positions
                    .iter()
                    .enumerate()
                    .map(|(i, &t)| NativeGreenNode {
                        absolute_position: t,
                        note_bpm: 120.,
                        note_index: if i == 1 { None } else { Some(i) },
                        role: if i == 0 {
                            GreenRole::Head
                        } else if i == 2 {
                            GreenRole::PlainEnd
                        } else {
                            GreenRole::HeldMiddle
                        },
                    })
                    .collect(),
            }],
            geometry: GreenGeometry {
                specific_speed: 10.,
                start_y: 4.9765005,
                button_world_y: -3.45,
                button_local_y: -3.45,
            },
        };
        let input = ReplayInput {
            power: 300040.75,
            play_level: 27,
            fps: 120,
            notes: (0..3)
                .map(|i| ReplayNote {
                    time: 4. + i as f64 / 2.,
                    trigger: None,
                    window: NoteWindow::PinnedPerfect {
                        frame: 480 + i * 60,
                    },
                })
                .collect(),
            hit_frames: vec![480, 540, 600],
            hit_order: vec![],
            skills: vec![],
            initial_combo: 0,
            medley_combo: false,
            fever_frames: None,
            constraints: vec![],
            native_time_routing: None,
        };
        (source, input)
    }
    #[test]
    fn roundtrip_reuses_only_exact_chart_source_and_installs_transactionally() {
        let (source, mut input) = fixture();
        let cache = NativeGreenChartCache::prepare(source.clone(), &input).unwrap();
        let encoded = serde_json::to_string(&cache).unwrap();
        let cache: NativeGreenChartCache = serde_json::from_str(&encoded).unwrap();
        assert!(cache.matches(&source, &input));
        input.power = 700000.5;
        assert!(cache.matches(&source, &input));
        cache.install(&mut input).unwrap();
        assert_eq!(cache.green_note_count(), 2);
        assert_eq!(replay(&input).unwrap().pinned_note_count, 1);
        let edges = input.constraints.len();
        cache.install(&mut input).unwrap();
        assert_eq!(input.constraints.len(), edges);
        let mut other = source.clone();
        other.geometry.start_y += 0.01;
        assert!(!cache.matches(&other, &input));
        input.notes[0].time += 0.01;
        let before = serde_json::to_value(&input).unwrap();
        assert!(!cache.matches(&source, &input));
        assert!(cache.install(&mut input).is_err());
        assert_eq!(before, serde_json::to_value(&input).unwrap());
        let mut malformed: serde_json::Value = serde_json::from_str(&encoded).unwrap();
        malformed["revision"] = "old".into();
        let cache: NativeGreenChartCache = serde_json::from_value(malformed).unwrap();
        assert!(!cache.matches(&source, &fixture().1));
        assert!(cache.install(&mut fixture().1).is_err());
    }
    #[test]
    fn prepared_domains_feed_all_skill_effects_corrections_and_compiled_scores() {
        use bangdream_optimize_core::{
            hit_timing::{
                compiled::TimingCompileCache, search::SearchOptions, JudgementCorrection,
                ReplaySkill, ScoreEffect,
            },
            scoring::{Judgement, ScoringPower},
        };
        let (source, mut input) = fixture();
        input.notes[1].trigger = Some(0);
        input.skills = vec![ReplaySkill {
            duration: 1.,
            score: ScoreEffect::Plain { percent: 110. },
            correction: None,
        }];
        let cache = NativeGreenChartCache::prepare(source, &input).unwrap();
        let effects = [
            ScoreEffect::Plain { percent: 110. },
            ScoreEffect::PerfectOnly { percent: 125. },
            ScoreEffect::PerfectOrZero { percent: 125. },
            ScoreEffect::PerfectOrHalf { percent: 125. },
            ScoreEffect::Continuous {
                high: 110.,
                low: 90.,
            },
            ScoreEffect::Crescendo {
                percent: 100.,
                step: 0.5,
                cap: 150,
            },
        ];
        for effect in effects {
            for correction in [None, Some(JudgementCorrection::Bad)] {
                input.skills[0].score = effect.clone();
                input.skills[0].correction = correction;
                cache.install(&mut input).unwrap();
                let mut bank = TimingCompileCache::new(&input, SearchOptions::default());
                let compiled = bank.prepare(&input.skills).unwrap();
                for power in [300040.75, 700000.5] {
                    let found = compiled
                        .materialize(ScoringPower::new(power).unwrap())
                        .unwrap();
                    let mut witness = input.clone();
                    witness.power = power;
                    witness.hit_frames = found.hit_frames.clone();
                    witness.hit_order = found.hit_order.clone();
                    assert_eq!(replay(&witness).unwrap().total, found.total);
                }
                // Native early Bad remains in the domain; only an active
                // correction repairs it. The source cache never strips it.
                let NoteWindow::NativeGreen { first_frames, .. } = input.notes[2].window else {
                    panic!()
                };
                let mut early = input.clone();
                early.hit_frames[2] = first_frames[0];
                if correction.is_some() {
                    let trace = replay(&early).unwrap().trace;
                    let end = trace.iter().find(|n| n.note_index == 2).unwrap();
                    assert_eq!(end.raw, Judgement::Bad);
                    assert_eq!(end.judgement, Judgement::Perfect);
                } else {
                    assert!(replay(&early).is_err());
                }
            }
        }
    }
}
