//! Optional same-lane candidate routing, using native coordinates or chart time.
//! Unmapped pinned nodes remain explicitly unverified, never time candidates.
//! Separate lanes admit independent contacts. Settled chart-time candidates
//! leave the press list; green-chain spacing is checked by explicit edges.
use super::{
    native_input::time_candidate_is_nearer, NoteWindow, ReplayError, ReplayInput, REPLAY_FPS,
};
use crate::scoring::Judgement;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// A time-judged scoring target. Settled targets leave the press list.
pub struct NativeTimeTarget {
    pub button: u8,
    pub absolute_pos: i32,
    /// Native-list membership in native mode; modeled Bad-window eligibility in chart-time mode.
    pub active_from_frame: u32,
    pub active_until_frame: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTimeRouting {
    #[serde(default, skip_serializing_if = "CandidateDistanceMode::is_native")]
    pub distance_mode: CandidateDistanceMode,
    /// Frame-indexed unadjusted native MusicPos, not seconds / adjusted grade time.
    pub music_positions: Vec<f32>,
    /// One entry per scoring note. Only unimplemented pinned nodes may be None.
    pub targets: Vec<Option<NativeTimeTarget>>,
    /// Actual relative order of native root objects; excludes release endpoints.
    pub candidate_order: Vec<usize>,
    /// Explicit native NoteLong -> plain endpoint links, never inferred from shape.
    #[serde(default)]
    pub connections: Vec<NativeTimeConnection>,
    /// Declared within-frame replay order, distinct from candidate-list order.
    /// Legacy baseline metadata. ReplayInput.hit_order overrides it; complete
    /// optimization searches the within-frame order and emits a separate witness.
    /// Empty retains the historical note-index order; otherwise a full permutation.
    #[serde(default)]
    pub event_order: Vec<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CandidateDistanceMode {
    #[default]
    NativeMusicPos,
    ChartTime,
}
impl CandidateDistanceMode {
    fn is_native(&self) -> bool {
        *self == Self::NativeMusicPos
    }
}

/// A strict improvement only. Equality preserves the earlier root in the
/// supplied candidate order, including for exact chart-time ties.
pub(crate) fn candidate_is_nearer(
    input: &ReplayInput,
    retained: usize,
    candidate: usize,
    frame: u32,
) -> bool {
    let r = input.native_time_routing.as_ref().unwrap();
    match r.distance_mode {
        CandidateDistanceMode::NativeMusicPos => time_candidate_is_nearer(
            r.targets[retained].as_ref().unwrap().absolute_pos,
            r.targets[candidate].as_ref().unwrap().absolute_pos,
            r.music_positions[frame as usize],
        ),
        CandidateDistanceMode::ChartTime => {
            let now = f64::from(frame) / f64::from(REPLAY_FPS);
            (now - input.notes[candidate].time).abs()
                < (now - input.notes[retained].time).abs()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeTimeConnection {
    pub head: usize,
    pub end: usize,
    /// Identity of the continuous contact from head press to endpoint release.
    pub finger_id: i32,
    /// Result of the native release-position hitbox check. No 2D hitbox is inferred.
    pub release_inside: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutingCoverage {
    pub time_candidate_notes: usize,
    /// These nodes can also interfere with time candidates. Their interactions
    /// have NOT been verified by the ordinary-candidate pass.
    pub unmapped_pinned_notes: Vec<usize>,
    /// Movable green domains without verified position-candidate routing.
    pub unmapped_green_notes: Vec<usize>,
    pub native_coordinates_supplied: bool,
    pub time_connection_count: usize,
}

pub(super) fn coverage(input: &ReplayInput) -> RoutingCoverage {
    RoutingCoverage {
        time_candidate_notes: input
            .native_time_routing
            .as_ref()
            .map_or(0, |r| r.candidate_order.len()),
        unmapped_pinned_notes: input
            .notes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                (matches!(n.window, NoteWindow::PinnedPerfect { .. })
                    && input
                        .native_time_routing
                        .as_ref()
                        .is_none_or(|r| r.targets[i].is_none()))
                .then_some(i)
            })
            .collect(),
        unmapped_green_notes: input
            .notes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| matches!(n.window, NoteWindow::NativeGreen { .. }).then_some(i))
            .collect(),
        native_coordinates_supplied: input.native_time_routing.as_ref().is_some_and(|r| {
            r.distance_mode == CandidateDistanceMode::NativeMusicPos
        }),
        time_connection_count: input
            .native_time_routing
            .as_ref()
            .map_or(0, |r| r.connections.len()),
    }
}

pub(super) fn validate(input: &ReplayInput) -> Result<(), ReplayError> {
    if !input.hit_order.is_empty() {
        let mut seen = vec![false; input.notes.len()];
        if input.hit_order.len() != seen.len() {
            return Err(ReplayError::Input("hit order must be a full permutation"));
        }
        for &i in &input.hit_order {
            if i >= seen.len() || seen[i] {
                return Err(ReplayError::Input("hit order must be a full permutation"));
            }
            seen[i] = true;
        }
    }
    let Some(r) = &input.native_time_routing else {
        return Ok(());
    };
    let n = input.notes.len();
    let chart_time = r.distance_mode == CandidateDistanceMode::ChartTime;
    if r.targets.len() != n
        || (chart_time && !r.music_positions.is_empty())
        || (!chart_time && r.music_positions.is_empty())
        || r.music_positions.len() > (super::MAX_SECONDS * REPLAY_FPS + 1) as usize
        || r.music_positions.iter().any(|p| !p.is_finite())
        || (chart_time && !r.connections.is_empty())
    {
        return Err(ReplayError::Input(
            "invalid native routing dimensions/positions",
        ));
    }
    let mut seen = vec![false; n];
    let mut linked = vec![false; n];
    let mut endpoints = vec![false; n];
    for c in &r.connections {
        if c.head >= n || c.end >= n || c.head == c.end || c.finger_id < 0 {
            return Err(ReplayError::Input("invalid native time connection"));
        }
        if linked[c.head] || linked[c.end] {
            return Err(ReplayError::Input("native time connections share a node"));
        }
        let (Some(head), Some(end)) = (&r.targets[c.head], &r.targets[c.end]) else {
            return Err(ReplayError::Input(
                "native time connection lacks target metadata",
            ));
        };
        if !matches!(input.notes[c.head].window, NoteWindow::Standard)
            || !matches!(input.notes[c.end].window, NoteWindow::WidenedEndpoint)
            || head.button != end.button
        {
            return Err(ReplayError::Input(
                "native time connection has wrong window/button",
            ));
        }
        linked[c.head] = true;
        linked[c.end] = true;
        endpoints[c.end] = true;
    }
    for &i in &r.candidate_order {
        if i >= n || seen[i] || r.targets[i].is_none() || endpoints[i] {
            return Err(ReplayError::Input(
                "native candidate order must contain each mapped root once",
            ));
        }
        seen[i] = true;
    }
    for (i, t) in r.targets.iter().enumerate() {
        if let Some(t) = t {
            if (!seen[i] && !endpoints[i])
                || t.button > 6
                || t.active_from_frame > t.active_until_frame
                || (chart_time && t.active_until_frame > super::MAX_SECONDS * REPLAY_FPS)
                || (!chart_time && t.active_until_frame as usize >= r.music_positions.len())
                || (!chart_time && !endpoints[i] && !matches!(input.notes[i].window, NoteWindow::Standard))
            {
                return Err(ReplayError::Input(
                    "invalid native press target/order/lifetime",
                ));
            }
        } else if !matches!(input.notes[i].window, NoteWindow::PinnedPerfect { .. }) {
            return Err(ReplayError::Input(
                "movable note lacks native routing metadata",
            ));
        }
    }
    if chart_time {
        for (i, target) in r.targets.iter().enumerate() {
            let Some(target) = target else {
                return Err(ReplayError::Input("chart-time target metadata must cover every note"));
            };
            if target.absolute_pos != 0 || !matches!(input.notes[i].window,
                NoteWindow::Standard | NoteWindow::WidenedEndpoint | NoteWindow::SlideTime { .. }) {
                return Err(ReplayError::Input("chart-time routing requires declared time targets"));
            }
            let note = &input.notes[i];
            let a = target.active_from_frame;
            let b = target.active_until_frame;
            if note.raw_judgement(a)? < Judgement::Bad
                || note.raw_judgement(b)? < Judgement::Bad
                || (a > 0 && note.raw_judgement(a - 1)? >= Judgement::Bad)
            {
                return Err(ReplayError::Input(
                    "chart-time candidate eligibility must start at Bad and end inside Bad",
                ));
            }
        }
    }
    if !r.event_order.is_empty() {
        let mut events_seen = vec![false; n];
        if r.event_order.len() != n {
            return Err(ReplayError::Input(
                "native event order must be a full permutation",
            ));
        }
        for &i in &r.event_order {
            if i >= n || events_seen[i] {
                return Err(ReplayError::Input(
                    "native event order must be a full permutation",
                ));
            }
            events_seen[i] = true;
        }
    }
    Ok(())
}

pub(super) fn event_ranks(input: &ReplayInput) -> Vec<usize> {
    let mut ranks: Vec<_> = (0..input.notes.len()).collect();
    if let Some(r) = &input.native_time_routing {
        for (rank, &i) in r.event_order.iter().enumerate() {
            ranks[i] = rank;
        }
    }
    for (rank, &i) in input.hit_order.iter().enumerate() {
        ranks[i] = rank;
    }
    ranks
}

/// Compile only per-button traversal, never candidate ownership. Ownership is
/// recomputed for EVERY proposal in actual input order, including within a frame.
pub(super) struct Router<'a> {
    input: &'a ReplayInput,
    routing: Option<&'a NativeTimeRouting>,
    by_button: [Vec<usize>; 7],
    consumed: Vec<bool>,
    heads: Vec<Option<usize>>,
    ends: Vec<Option<usize>>,
    bound: Vec<bool>,
    /// GamePlayButton owns this lookup per button, not a global finger lock.
    touch_began: [HashMap<i32, usize>; 7],
}
impl<'a> Router<'a> {
    pub fn new(input: &'a ReplayInput) -> Self {
        let mut by_button = std::array::from_fn(|_| Vec::new());
        let mut heads = vec![None; input.notes.len()];
        let mut ends = vec![None; input.notes.len()];
        if let Some(r) = &input.native_time_routing {
            for &i in &r.candidate_order {
                by_button[r.targets[i].as_ref().unwrap().button as usize].push(i);
            }
            for (i, c) in r.connections.iter().enumerate() {
                heads[c.head] = Some(i);
                ends[c.end] = Some(i);
            }
        }
        Self {
            input,
            routing: input.native_time_routing.as_ref(),
            by_button,
            consumed: vec![false; input.notes.len()],
            heads,
            ends,
            bound: vec![
                false;
                input
                    .native_time_routing
                    .as_ref()
                    .map_or(0, |r| r.connections.len())
            ],
            touch_began: std::array::from_fn(|_| HashMap::new()),
        }
    }
    pub fn reset(&mut self) {
        self.consumed.fill(false);
        self.bound.fill(false);
        for button in &mut self.touch_began {
            button.clear();
        }
    }
    pub fn press(&mut self, note: usize, frame: u32) -> Result<(), ReplayError> {
        let Some(r) = self.routing else {
            return Ok(());
        };
        let Some(target) = &r.targets[note] else {
            return Ok(());
        };
        if let Some(connection) = self.ends[note] {
            let c = &r.connections[connection];
            let head = r.targets[c.head].as_ref().unwrap();
            // GamePlayButton.ExecTouchEnded gets the bound root by finger ID.
            // It does not run the nearest-candidate selector a second time.
            if !self.bound[connection]
                || self.consumed[note]
                || self.touch_began[head.button as usize].get(&c.finger_id) != Some(&connection)
                || frame < head.active_from_frame
                || frame > head.active_until_frame
                || frame < target.active_from_frame
                || frame > target.active_until_frame
            {
                return Err(ReplayError::InputBinding {
                    index: note,
                    frame,
                    reason: "release has no active bound head",
                });
            }
            if !c.release_inside {
                return Err(ReplayError::InputBinding {
                    index: note,
                    frame,
                    reason: "plain endpoint release is outside its target button",
                });
            }
            self.bound[connection] = false;
            self.touch_began[head.button as usize].remove(&c.finger_id);
            self.consumed[c.head] = true;
            self.consumed[note] = true;
            return Ok(());
        }
        if r.distance_mode == CandidateDistanceMode::NativeMusicPos
            && r.music_positions.get(frame as usize).is_none()
        {
            return Err(ReplayError::Input("native MusicPos samples do not cover candidate frame"));
        }
        let mut selected: Option<usize> = None;
        for &i in &self.by_button[target.button as usize] {
            let c = r.targets[i].as_ref().unwrap();
            if self.consumed[i]
                || frame < c.active_from_frame
                || frame > c.active_until_frame
            {
                continue;
            }
            if selected.is_none_or(|old| candidate_is_nearer(self.input, old, i, frame)) {
                selected = Some(i);
            }
        }
        if selected != Some(note) {
            return Err(ReplayError::InputTarget {
                index: note,
                frame,
                selected,
            });
        }
        if let Some(connection) = self.heads[note] {
            let c = &r.connections[connection];
            // Retain endpoint ownership independently of press eligibility.
            // NoteLong endpoint binding is separate from press eligibility.
            self.bound[connection] = true;
            // Native setTouchBeganNote overwrites this button's slot. It does
            // not globally reject another root carrying the same finger ID.
            self.touch_began[target.button as usize].insert(c.finger_id, connection);
        }
        // Current plans require an explicit, successful press for each target.
        // Its raw grade is checked by the caller before this plan can be accepted.
        self.consumed[note] = true;
        Ok(())
    }
}
