//! Explicit manual-input replay for timing research. Existing calculation modes
//! do not call this interface. Frames process inputs in explicit dispatch order
//! (note-index order by default) before one skill-manager update. This is a
//! declared model, not device emulation.
pub mod bounds;
#[cfg(test)]
mod chain_tests;
mod clock;
pub mod compiled;
mod constraints;
pub mod enumeration;
pub mod green;
pub mod independent;
pub mod layouts;
pub mod lazy_medley;
pub mod lazy_orders;
pub mod native_input;
pub mod ranges;
mod reference;
mod note_window;
#[cfg(test)]
mod typed_windows_tests;
mod routing;
mod schema;
pub mod search;
mod skill;
#[cfg(test)]
mod time_connections;
mod witness;
mod baseline;

pub use routing::{CandidateDistanceMode, NativeTimeConnection, NativeTimeRouting, NativeTimeTarget, RoutingCoverage};
pub use schema::*;

use crate::scoring::{Judgement, NoteScorer, ScoringError, ScoringPower};
use clock::SkillClock;
use thiserror::Error;

pub const TIMING_REVISION: &str = "typed-time-windows-v13";
pub const REPLAY_FPS: u32 = 120;
const MAX_SECONDS: u32 = 600;

#[derive(Debug, Error, PartialEq)]
pub enum ReplayError {
    #[error("invalid replay input: {0}")]
    Input(&'static str),
    #[error("invalid skill in slot {index}: {reason}")]
    Skill { index: usize, reason: &'static str },
    #[error("invalid note {index}: {reason}")]
    Note { index: usize, reason: &'static str },
    #[error("note {index} has disallowed judgment {raw:?} -> {corrected:?}")]
    Judgement {
        index: usize,
        raw: Judgement,
        corrected: Judgement,
    },
    #[error("skill-note order was changed")]
    SkillOrder,
    #[error("input for note {index} at frame {frame} selects {selected:?} instead")]
    InputTarget {
        index: usize,
        frame: u32,
        selected: Option<usize>,
    },
    #[error("input for note {index} at frame {frame}: {reason}")]
    InputBinding {
        index: usize,
        frame: u32,
        reason: &'static str,
    },
    #[error("hit plan violates constraint {index}")]
    Constraint { index: usize },
    #[error("replay clock exceeded its completion bound")]
    ClockLimit,
    #[error(transparent)]
    Scoring(#[from] ScoringError),
}

/// Native fixed 1/60-second judgment units, independent of input sampling FPS.
/// The quotient is rounded to even, not Rust's away-from-zero `round`.
pub fn time_judgement(delta: f64, widened_endpoint: bool) -> Result<Judgement, ReplayError> {
    if !delta.is_finite() {
        return Err(ReplayError::Input("judgment offset must be finite"));
    }
    let units = ((delta.abs() as f32) / (1.0_f32 / 60.0_f32)).round_ties_even();
    let extra = if widened_endpoint { 1.0 } else { 0.0 };
    for (boundary, grade) in [
        (3.0, Judgement::Perfect),
        (6.0, Judgement::Great),
        (7.0, Judgement::Good),
        (8.0, Judgement::Bad),
    ] {
        if units < boundary + extra {
            return Ok(grade);
        }
    }
    Ok(Judgement::Miss)
}

/// Fully specified fixed-order replay. Uncorrected Good/Bad and all Misses
/// reject the plan; this interface neither searches nor assumes future skills.
pub fn replay(input: &ReplayInput) -> Result<ReplayResult, ReplayError> {
    validate(input)?;
    let count =
        u32::try_from(input.notes.len()).map_err(|_| ReplayError::Input("too many notes"))?;
    let power = ScoringPower::new(input.power)?;
    let scorer = NoteScorer::new(power, input.play_level, count)?;
    let mut order: Vec<_> = (0..input.notes.len()).collect();
    let event_ranks = routing::event_ranks(input);
    order.sort_unstable_by_key(|&i| (input.hit_frames[i], event_ranks[i]));
    let requested: Vec<_> = order
        .iter()
        .filter_map(|&i| input.notes[i].trigger)
        .collect();
    if requested.windows(2).any(|p| p[0] >= p[1]) {
        return Err(ReplayError::SkillOrder);
    }

    let mut clock = SkillClock::new(input.fps);
    let mut router = routing::Router::new(input);
    let mut trace = Vec::with_capacity(order.len());
    let mut cursor = 0;
    let mut combo = input.initial_combo;
    let mut total = 0u64;
    let last = *input.hit_frames.iter().max().unwrap();
    // Includes every requested skill, cooldown and transition update. The
    // validated six-slot / 60-second duration limits keep arithmetic bounded.
    let tail = (input
        .skills
        .iter()
        .map(|s| f64::from(s.duration) + 1.0)
        .sum::<f64>()
        * f64::from(input.fps))
    .ceil() as u32
        + 10;
    for frame in 0..=last + tail {
        while cursor < order.len() && input.hit_frames[order[cursor]] == frame {
            let index = order[cursor];
            let note = &input.notes[index];
            router.press(index, frame)?;
            let time = f64::from(frame) / f64::from(input.fps);
            let raw = note.raw_judgement(frame)?;
            let active_slot = clock.active_index();
            let corrected = clock.active_mut().map_or(raw, |s| s.correct(raw));
            if corrected < Judgement::Great {
                return Err(ReplayError::Judgement {
                    index,
                    raw,
                    corrected,
                });
            }
            combo = combo
                .checked_add(1)
                .ok_or(ReplayError::Input("combo overflow"))?;
            let multiplier = clock.active_mut().map_or(1.0, |s| s.multiplier(corrected));
            let state = clock.active_mut().map(|s| s.snapshot());
            let fever = if input
                .fever_frames
                .is_some_and(|[start, end]| start <= frame && frame < end)
            {
                2.0
            } else {
                1.0
            };
            let score =
                scorer.score(corrected, input.combo_multiplier(combo), multiplier, fever)?;
            total = total
                .checked_add(u64::from(score.final_score))
                .ok_or(ScoringError::TotalScoreOverflow)?;
            trace.push(HitTrace {
                note_index: index,
                frame,
                hit_time: time,
                offset_ms: (time - note.time) * 1000.0,
                raw,
                judgement: corrected,
                combo,
                pinned_perfect: matches!(note.window, NoteWindow::PinnedPerfect { .. }),
                active_skill: active_slot,
                skill_multiplier: multiplier,
                skill_state: state,
                fever_multiplier: fever,
                before_skill: score.before_skill,
                score: score.final_score,
            });
            if let Some(slot) = note.trigger {
                clock.request(slot, frame);
            }
            cursor += 1;
        }
        clock.after_hits(frame, &input.skills)?;
        if cursor == order.len() && clock.finished() {
            return Ok(ReplayResult {
                scoring_revision: crate::scoring::ARITHMETIC_REVISION.to_owned(),
                timing_revision: TIMING_REVISION.to_owned(),
                routing_coverage: routing::coverage(input),
                fps: input.fps,
                scoring_power: power.value(),
                total,
                trace,
                skill_events: clock.events,
                hit_frames: input.hit_frames.clone(),
                hit_order: order.clone(),
                pinned_note_count: input
                    .notes
                    .iter()
                    .filter(|n| matches!(n.window, NoteWindow::PinnedPerfect { .. }))
                    .count(),
            });
        }
    }
    Err(ReplayError::ClockLimit)
}

fn validate(input: &ReplayInput) -> Result<(), ReplayError> {
    if input.fps != REPLAY_FPS {
        return Err(ReplayError::Input("hit-timing replay requires 120 FPS"));
    }
    if input.notes.is_empty() || input.notes.len() != input.hit_frames.len() {
        return Err(ReplayError::Input(
            "matching nonempty notes/hits are required",
        ));
    }
    // The archived native Medley master table covers [0, 9999]. Outside that
    // table the native lookup falls back to 1.0, whereas the inherited formula
    // stays at 1.34. Keep the optimizer inside the proven monotone Combo domain.
    if input.medley_combo
        && (input.initial_combo > 9_999
            || input.notes.len() > (9_999 - input.initial_combo) as usize)
    {
        return Err(ReplayError::Input(
            "Medley Combo exceeds the verified master-table range [0, 9999]",
        ));
    }
    if input.skills.len() > 6 {
        return Err(ReplayError::Input(
            "at most six fixed skill slots are supported",
        ));
    }
    if input
        .hit_frames
        .iter()
        .any(|&f| f > MAX_SECONDS * input.fps)
    {
        return Err(ReplayError::Input(
            "hit frame exceeds the 600-second replay limit",
        ));
    }
    if input.fever_frames.is_some_and(|[start, end]| start >= end) {
        return Err(ReplayError::Input(
            "Fever requires an explicit nonempty [start, end) interval",
        ));
    }
    for (index, skill) in input.skills.iter().enumerate() {
        skill.validate(index)?;
    }
    let mut triggers = [false; 6];
    for (index, note) in input.notes.iter().enumerate() {
        if !note.time.is_finite() || note.time < 0.0 {
            return Err(ReplayError::Note {
                index,
                reason: "note time must be finite and nonnegative",
            });
        }
        note.validate_window()?;
        if let NoteWindow::PinnedPerfect { frame } = note.window {
            if frame != input.hit_frames[index] {
                return Err(ReplayError::Note {
                    index,
                    reason: "unverified pinned node cannot move",
                });
            }
        }
        if let Some(slot) = note.trigger {
            if slot >= input.skills.len() || triggers[slot] {
                return Err(ReplayError::Note {
                    index,
                    reason: "skill slots must exist and be requested at most once",
                });
            }
            triggers[slot] = true;
        }
    }
    // Validate the explicit order before indexing its ranks in causal edges.
    routing::validate(input)?;
    let event_ranks = input
        .constraints
        .iter()
        .any(|c| c.require_dispatch_order)
        .then(|| routing::event_ranks(input));
    for (index, c) in input.constraints.iter().enumerate() {
        let n = input.notes.len();
        if c.before >= n || c.after >= n || c.before == c.after {
            return Err(ReplayError::Input("constraint references an invalid note"));
        }
        let a = &input.notes[c.before];
        let b = &input.notes[c.after];
        if a.time > b.time || (a.time == b.time && c.before >= c.after) {
            return Err(ReplayError::Input(
                "constraint must follow stable chronological note order",
            ));
        }
        if let Some(curve) = &c.earliest_after {
            curve.validate()?;
        }
        if !c.allows_frames(input.hit_frames[c.before], input.hit_frames[c.after])
            || (c.require_dispatch_order
                && event_ranks.as_ref().is_some_and(|r| {
                    (input.hit_frames[c.before], r[c.before])
                        >= (input.hit_frames[c.after], r[c.after])
                }))
        {
            return Err(ReplayError::Constraint { index });
        }
    }
    Ok(())
}

#[cfg(test)]
mod native_green_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod touch_order_tests;
