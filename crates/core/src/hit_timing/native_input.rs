//! CN 9.4.4 candidate selection. Callers supply the active list in its native
//! traversal order. MusicPos, transform Y and virtual judgment Y are distinct.
//! This module does not infer those values from seconds or render coordinates.
use crate::scoring::Judgement;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CandidateKind {
    Time { absolute_pos: i32 },
    Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Candidate {
    pub id: usize,
    pub button: u8,
    pub kind: CandidateKind,
    /// Transform.position.y, NOT clamped VirtualPosY.
    pub world_y: f32,
    pub finger_id: i32,
    /// None is native None (-1); correction cannot turn it into a valid press.
    pub raw: Option<Judgement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Dispatch {
    InvalidCoordinates,
    Empty,
    NoJudgement { id: usize },
    FingerBound { id: usize },
    Hit { id: usize, raw: Judgement },
}

/// NoteManager 0x189fca8..0x189fcf4: two integer AbsolutePos values are
/// converted to f32 BEFORE subtracting the f32 MusicPos; ties retain A.
pub fn time_candidate_is_nearer(retained: i32, candidate: i32, music_pos: f32) -> bool {
    ((candidate as f32) - music_pos).abs() < ((retained as f32) - music_pos).abs()
}

/// SlideNoteManager 0x2150b9c..0x2150bbc: ties return the first argument.
pub fn position_first_is_nearer(a_y: f32, b_y: f32, line_y: f32) -> bool {
    (a_y - line_y).abs() <= (b_y - line_y).abs()
}

/// Literal native list traversal, followed by raw-grade and finger checks.
/// A rejected winner does not cause a retry with the second-best candidate.
pub fn dispatch(candidates: &[Candidate], button: u8, music_pos: f32, line_y: f32) -> Dispatch {
    if !music_pos.is_finite()
        || !line_y.is_finite()
        || candidates.iter().any(|c| !c.world_y.is_finite())
    {
        return Dispatch::InvalidCoordinates;
    }
    let (mut time, mut position): (Option<&Candidate>, Option<&Candidate>) = (None, None);
    for c in candidates.iter().filter(|c| c.button == button) {
        match c.kind {
            CandidateKind::Time { absolute_pos } => {
                let replace = time.is_none_or(|old| {
                    let CandidateKind::Time {
                        absolute_pos: previous,
                    } = old.kind
                    else {
                        unreachable!()
                    };
                    time_candidate_is_nearer(previous, absolute_pos, music_pos)
                });
                if replace {
                    time = Some(c);
                }
            }
            CandidateKind::Position => {
                // Native checks binding only when seeding the position list.
                // Later candidates compete first; the winner is checked again
                // by GamePlayButton. Do not filter all bound objects up front.
                if position.is_none() {
                    if c.finger_id <= -1 {
                        position = Some(c);
                    }
                } else if !position_first_is_nearer(position.unwrap().world_y, c.world_y, line_y) {
                    position = Some(c);
                }
            }
        }
    }
    let winner = match (time, position) {
        (Some(a), Some(b)) if !position_first_is_nearer(a.world_y, b.world_y, line_y) => Some(b),
        (Some(a), _) => Some(a),
        (_, b) => b,
    };
    let Some(winner) = winner else {
        return Dispatch::Empty;
    };
    match winner.raw {
        None => Dispatch::NoJudgement { id: winner.id },
        Some(_) if winner.finger_id > -1 => Dispatch::FingerBound { id: winner.id },
        Some(raw) => Dispatch::Hit { id: winner.id, raw },
    }
}

/// NoteSlide.WaitState 0x1e8ce1c..0x1e8ce44. Only the midpoint branch,
/// after earlier timeout/state checks and with a known next visible node.
pub fn connection_head_midpoint_expired(current: f32, head: i32, next: i32) -> bool {
    let elapsed = current - head as f32;
    let remaining = next as f32 - current;
    remaining <= 0.0 || elapsed > remaining
}

/// NoteSlideAfter.StopState 0x214b0d8..0x214b100. Caller must have passed
/// state/adjustment-delay guards. Nonpositive remaining is NOT this branch.
pub fn connection_after_midpoint_expired(current: f32, node: i32, next: i32) -> bool {
    let elapsed = current - node as f32;
    let remaining = next as f32 - current;
    remaining > 0.0 && elapsed > remaining
}

#[cfg(test)]
mod tests;
