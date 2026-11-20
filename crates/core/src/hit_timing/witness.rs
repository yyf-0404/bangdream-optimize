//! Small, reusable family of complete Perfect plans. Every plan is routed and
//! replayed; these are feasible witnesses, never a claim of timing optimality.
use super::*;
use std::collections::BTreeSet;

pub(super) fn perfect(input: &ReplayInput, domains: &[Vec<u32>]) -> Vec<ReplayResult> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for variant in 0..3 {
        let frames: Option<Vec<_>> = domains
            .iter()
            .enumerate()
            .map(|(i, fs)| {
                fs.iter()
                    .copied()
                    .filter(|&frame| input.notes[i].raw_judgement(frame) == Ok(Judgement::Perfect))
                    .min_by_key(|&frame| {
                        (
                            match variant {
                                0 => frame.abs_diff(input.hit_frames[i]),
                                1 => frame,
                                _ => u32::MAX - frame,
                            },
                            frame,
                        )
                    })
            })
            .collect();
        let Some(frames) = frames else { continue };
        if !seen.insert(frames.clone()) {
            continue;
        }
        let mut neutral = input.clone();
        neutral.hit_frames = frames;
        neutral.skills = vec![
            ReplaySkill {
                duration: 0.01,
                score: ScoreEffect::Plain { percent: 0. },
                correction: None
            };
            input.skills.len()
        ];
        if let Ok(replayed) = replay(&neutral) {
            if replayed.trace.iter().all(|t| t.raw == Judgement::Perfect) {
                result.push(replayed);
            }
        }
    }
    result
}
