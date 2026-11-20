//! Declared 120 FPS chart-time model over visible Bestdori scoring nodes.
//! Hidden-node state, spatial judgment and speed settings are not inputs.
use crate::DataError;
use bangdream_optimize_core::{
    hit_timing::{
        replay, CandidateDistanceMode, NativeTimeRouting, NativeTimeTarget,
        NoteConstraint, NoteWindow, ReplayError, ReplayInput, ReplayNote, ReplaySkill, SlideTimeRole, REPLAY_FPS,
    },
    scoring::Judgement,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

mod tempo;
use tempo::{BpmSegment, TempoMap};
mod lifetime_scan;
use lifetime_scan::{midpoint_expiry, timeout_last_frames, TimeoutNode};

pub const TIME_DISTANCE_PREPARATION_REVISION: &str = "visible-typed-time-midpoint-120-v6";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum VisibleRole {
    Independent,
    Long,
    SlideHead,
    SlideMiddle,
    SlideEnd,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleNoteIdentity {
    pub id: String,
    pub beat: f64,
    pub lane: u8,
    pub role: VisibleRole,
}

#[derive(Debug, Clone)]
struct Point {
    identity: VisibleNoteIdentity,
    event: usize,
    node: usize,
    skill: bool,
    window: NoteWindow,
    previous: Option<String>,
    next: Option<String>,
}

/// Immutable, card-independent preparation. Constructed only from raw chart
/// data; no deserialization of unchecked cached deadlines is exposed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeDistanceChart {
    revision: &'static str,
    basic_bpm: f32,
    bpm_segments: Vec<BpmSegment>,
    identities: Vec<VisibleNoteIdentity>,
    notes: Vec<ReplayNote>,
    centered_frames: Vec<u32>,
    constraints: Vec<NoteConstraint>,
    routing: NativeTimeRouting,
    fever_frames: Option<[u32; 2]>,
    head_midpoint_cutoffs: usize,
    middle_midpoint_cutoffs: usize,
    middle_successor_cutoffs: usize,
}

/// Score context supplied after shared chart preparation.
pub struct TimeDistanceScoring {
    pub power: f32,
    pub play_level: i32,
    pub skills: Vec<ReplaySkill>,
    pub initial_combo: u32,
    pub medley_combo: bool,
    pub fever: bool,
}

fn invalid(field: &'static str, value: impl Into<String>) -> DataError {
    DataError::InvalidField {
        field,
        value: value.into(),
    }
}
fn number(value: &Value, field: &'static str) -> Result<f64, DataError> {
    value
        .get(field)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid(field, "expected finite number"))
}
fn flag(value: &Value, field: &'static str) -> Result<bool, DataError> {
    match value.get(field) {
        None => Ok(false),
        Some(Value::Bool(v)) => Ok(*v),
        _ => Err(invalid(field, "expected boolean")),
    }
}
fn tick(beat: f64, source: &str) -> Result<i32, DataError> {
    let value = beat * 48.;
    let rounded = value.round();
    if !value.is_finite()
        || value < 0.
        || rounded > f64::from(i32::MAX)
        || (value - rounded).abs() > 48e-9
    {
        return Err(invalid(
            "beat",
            format!("{source} needs an integer 1/48 beat tick"),
        ));
    }
    Ok(rounded as i32)
}

/// The time has already passed ReplayNote validation. U_q advances Move to
/// q+1 and switches only at q+1 > x, x=f64(time*120). The first Wait/Stop
/// handler is U_s, s=floor(x)+1; it reads clocks[s+1]. In particular neither
/// the equality update nor the state-switch update can expire the node.
fn first_slide_lifetime_clock(time: f64) -> u32 {
    (time * f64::from(REPLAY_FPS)).floor() as u32 + 2
}

impl TimeDistanceChart {
    pub fn from_bestdori(raw: &Value) -> Result<Self, DataError> {
        let events = raw
            .as_array()
            .filter(|a| !a.is_empty())
            .ok_or_else(|| invalid("chart", "expected nonempty event array"))?;
        let mut tempo = TempoMap::new(events)?;
        let basic_bpm = tempo.0[0].bpm as f32;
        let mut points = Vec::new();
        let mut fever = [None, None];
        for (event, row) in events.iter().enumerate() {
            let kind = row["type"]
                .as_str()
                .ok_or_else(|| invalid("type", "missing event type"))?;
            match kind {
                "BPM" => continue,
                "System" => {
                    let time = tempo.seconds(number(row, "beat")?)?;
                    let index = match row["data"].as_str() {
                        Some("cmd_fever_start.wav") => Some(0),
                        Some("cmd_fever_end.wav") => Some(1),
                        _ => None,
                    };
                    if let Some(i) = index {
                        if fever[i].replace(time).is_some() {
                            return Err(invalid("fever", "duplicate marker"));
                        }
                    }
                    continue;
                }
                "Single" | "Directional" | "Long" | "Slide" => {}
                _ => return Err(invalid("type", format!("unsupported event {kind}"))),
            }
            let connected = matches!(kind, "Long" | "Slide");
            let nodes = if connected {
                row["connections"]
                    .as_array()
                    .filter(|a| a.len() >= 2)
                    .ok_or_else(|| invalid("connections", "expected at least two nodes"))?
                    .as_slice()
            } else {
                std::slice::from_ref(row)
            };
            if kind == "Long" && nodes.len() != 2 {
                return Err(invalid("connections", "Long needs exactly a head and a tail"));
            }
            let mut chain = Vec::<Point>::new();
            for (node, value) in nodes.iter().enumerate() {
                if flag(value, "hidden")? {
                    if !connected || node == 0 || node + 1 == nodes.len() || flag(value, "skill")? {
                        return Err(invalid(
                            "hidden",
                            "hidden endpoints or scoring nodes are unsupported",
                        ));
                    }
                    // Interior hidden-node beat, lane and state do not affect
                    // this model. Only the visible subsequence is inspected.
                    continue;
                }
                let beat = number(value, "beat")?;
                tempo.seconds(beat)?;
                let lane = number(value, "lane")?;
                if !(0. ..=6.).contains(&lane) || lane.fract() != 0. {
                    return Err(invalid("lane", "expected visible lane 0..6"));
                }
                let role = if kind == "Slide" {
                    if node == 0 {
                        VisibleRole::SlideHead
                    } else if node + 1 == nodes.len() {
                        VisibleRole::SlideEnd
                    } else {
                        VisibleRole::SlideMiddle
                    }
                } else if kind == "Long" {
                    VisibleRole::Long
                } else {
                    VisibleRole::Independent
                };
                let flick = flag(value, "flick")?;
                let directional = match value.get("direction") {
                    None | Some(Value::Null) => false,
                    Some(Value::String(d)) if matches!(d.as_str(), "Left" | "Right") => true,
                    _ => return Err(invalid("direction", "expected Left or Right")),
                };
                if connected && node + 1 != nodes.len() && (flick || directional) {
                    return Err(invalid("connections", "only a connected tail can flick"));
                }
                if kind == "Long" && directional {
                    return Err(invalid("direction", "directional Long tail mapping is not verified"));
                }
                let window = match role {
                    VisibleRole::SlideHead => NoteWindow::SlideTime { role: SlideTimeRole::Head, last_frame:None },
                    VisibleRole::SlideMiddle => NoteWindow::SlideTime { role: SlideTimeRole::Middle, last_frame:None },
                    VisibleRole::SlideEnd => NoteWindow::SlideTime { role: if directional {
                        SlideTimeRole::DirectionalEnd
                    } else if flick { SlideTimeRole::FlickEnd } else { SlideTimeRole::End }, last_frame:None },
                    VisibleRole::Long if node + 1 == nodes.len() => NoteWindow::WidenedEndpoint,
                    _ => NoteWindow::Standard,
                };
                if connected && chain.last().is_some_and(|p| p.identity.beat >= beat) {
                    return Err(invalid(
                        "connections",
                        "visible beats must increase strictly",
                    ));
                }
                let id = format!("{event}:{node}");
                let previous = chain
                    .last()
                    .filter(|_| connected)
                    .map(|p| p.identity.id.clone());
                if let Some(p) = chain.last_mut().filter(|_| connected) {
                    p.next = Some(id.clone());
                }
                chain.push(Point {
                    identity: VisibleNoteIdentity {
                        id,
                        beat,
                        lane: lane as u8,
                        role,
                    },
                    event,
                    node,
                    skill: flag(value, "skill")?,
                    window,
                    previous,
                    next: None,
                });
            }
            points.extend(chain);
        }
        points.sort_by(|a, b| {
            a.identity
                .beat
                .total_cmp(&b.identity.beat)
                .then(a.event.cmp(&b.event))
                .then(a.node.cmp(&b.node))
        });
        if points.is_empty() {
            return Err(invalid("chart", "no visible scoring notes"));
        }
        let indices: BTreeMap<_, _> = points
            .iter()
            .enumerate()
            .map(|(i, p)| (p.identity.id.clone(), i))
            .collect();
        let mut slot = 0;
        let mut notes = Vec::new();
        let mut centered_frames = Vec::new();
        let mut constraints = Vec::new();
        let mut domains = Vec::new();
        for (i, p) in points.iter().enumerate() {
            let time = tempo.seconds(p.identity.beat)?;
            let trigger = p.skill.then(|| {
                let result = slot;
                slot += 1;
                result
            });
            let note = ReplayNote {
                time,
                trigger,
                window: p.window,
            };
            let domain = note.frame_domain(Judgement::Bad)?;
            let first = *domain
                .first()
                .ok_or_else(|| invalid("note", "empty Bad domain"))?;
            domains.push((first, *domain.last().unwrap()));
            let mut frame = (time * f64::from(REPLAY_FPS)).ceil() as u32;
            if let Some(previous) = &p.previous {
                let before = indices[previous];
                frame = frame.max(centered_frames[before]);
                constraints.push(NoteConstraint {
                    before,
                    after: i,
                    // The visible predecessor must be dispatched first, but
                    // adjacent nodes may share an input frame.
                    minimum_gap_frames: 0,
                    require_dispatch_order: true,
                    earliest_after: None,
                });
            }
            notes.push(note);
            centered_frames.push(frame);
        }
        // A BPM change can extend a timeout beyond t+M. Prepare the complete
        // supported horizon instead of clipping it to an assumed millisecond
        // window; the first native timeout remains final even if BPM changes.
        let clocks = tempo.music_clocks(600 * REPLAY_FPS)?;
        let timeout_nodes = points.iter().zip(&notes).filter_map(|(p, note)| {
            matches!(note.window, NoteWindow::SlideTime { role, .. } if role != SlideTimeRole::FlickEnd)
                .then(|| Ok(TimeoutNode { first_clock: first_slide_lifetime_clock(note.time),
                    absolute_pos: tick(p.identity.beat, "visible Slide node")? }))
        }).collect::<Result<Vec<_>, DataError>>()?;
        let mut timeouts = timeout_last_frames(&clocks, &timeout_nodes)?.into_iter();
        for i in 0..notes.len() {
            if let NoteWindow::SlideTime { role, .. } = notes[i].window {
                let last = if role == SlideTimeRole::FlickEnd { domains[i].1 } else {
                    timeouts.next().expect("one deadline per non-flick Slide node")
                };
                if last < domains[i].0 { return Err(invalid("timeout", "empty slide lifetime")); }
                domains[i].1 = last;
                notes[i].window = NoteWindow::SlideTime { role, last_frame:Some(last) };
            }
        }
        let mut head_midpoint_cutoffs = 0;
        let mut middle_midpoint_cutoffs = 0;
        let mut middle_successor_cutoffs = 0;
        let mut targets = Vec::new();
        for (i, (p, &(first, mut last))) in points.iter().zip(&domains).enumerate() {
            if matches!(
                p.identity.role,
                VisibleRole::SlideHead | VisibleRole::SlideMiddle
            ) {
                let current = tick(p.identity.beat, "visible Slide node")? as f32;
                let next_index = indices[p.next.as_ref().unwrap()];
                let next = tick(
                    points[next_index].identity.beat,
                    "visible Slide node",
                )? as f32;
                let expiry = midpoint_expiry(&clocks, first_slide_lifetime_clock(notes[i].time),
                    last, current, next, p.identity.role == VisibleRole::SlideHead);
                if let Some(expiry) = expiry {
                    if expiry == first {
                        return Err(invalid(
                            "midpoint",
                            "visible node has an empty candidate lifetime",
                        ));
                    }
                    last = expiry - 1;
                    if p.identity.role == VisibleRole::SlideHead {
                        head_midpoint_cutoffs += 1;
                    } else {
                        middle_midpoint_cutoffs += 1;
                    }
                }
                if p.identity.role == VisibleRole::SlideMiddle {
                    // While this node is unhit, dispatch order requires its
                    // successor to be unhit too. Updates visit the source
                    // chain forwards, so U_s(next) first observes that next
                    // node's Stop from U_(s(next)-1). Recall occurs AFTER
                    // I_s(next). This also covers skipped midpoint samples;
                    // it imposes a deadline, not a positive inter-hit gap.
                    let recall_last = first_slide_lifetime_clock(notes[next_index].time) - 1;
                    if recall_last < last {
                        last = recall_last;
                        middle_successor_cutoffs += 1;
                    }
                }
            }
            targets.push(Some(NativeTimeTarget {
                button: p.identity.lane,
                absolute_pos: 0,
                active_from_frame: first,
                active_until_frame: last,
            }));
        }
        let fever_frames = match fever {
            [None, None] => None,
            [Some(start), Some(end)] if start <= end => Some([
                (start * 120. - 1e-9).ceil().max(0.) as u32,
                (end * 120. + 1e-9).floor().max(0.) as u32,
            ]),
            _ => {
                return Err(invalid(
                    "fever",
                    "need an ordered pair of start/end markers",
                ))
            }
        };
        Ok(Self {
            revision: TIME_DISTANCE_PREPARATION_REVISION,
            basic_bpm,
            bpm_segments: tempo.0,
            identities: points.into_iter().map(|p| p.identity).collect(),
            notes,
            centered_frames,
            constraints,
            routing: NativeTimeRouting {
                distance_mode: CandidateDistanceMode::ChartTime,
                music_positions: vec![],
                targets,
                candidate_order: (0..indices.len()).collect(),
                connections: vec![],
                event_order: vec![],
            },
            fever_frames,
            head_midpoint_cutoffs,
            middle_midpoint_cutoffs,
            middle_successor_cutoffs,
        })
    }

    pub fn note_count(&self) -> usize {
        self.notes.len()
    }
    pub fn skill_count(&self) -> usize {
        self.notes.iter().filter(|n| n.trigger.is_some()).count()
    }
    pub fn identities(&self) -> &[VisibleNoteIdentity] {
        &self.identities
    }

    /// Produce a freshly replayed, all-Perfect feasible baseline. A failure to
    /// construct a baseline is explicit; it is not a claim of chart infeasibility.
    pub fn input(&self, scoring: TimeDistanceScoring) -> Result<ReplayInput, ReplayError> {
        let mut input = ReplayInput {
            power: scoring.power,
            play_level: scoring.play_level,
            fps: REPLAY_FPS,
            notes: self.notes.clone(),
            hit_frames: self.centered_frames.clone(),
            hit_order: vec![],
            skills: scoring.skills,
            initial_combo: scoring.initial_combo,
            medley_combo: scoring.medley_combo,
            fever_frames: if scoring.fever {
                self.fever_frames
            } else {
                None
            },
            constraints: self.constraints.clone(),
            native_time_routing: Some(self.routing.clone()),
        };
        if perfect_baseline(&input) {
            return Ok(input);
        }
        // Dense chains can need an earlier start than ceil(chart time). Try the
        // earliest Perfect frames before reporting that no seed was constructed.
        for (i, note) in input.notes.iter().enumerate() {
            input.hit_frames[i] = *note
                .frame_domain(Judgement::Perfect)?
                .first()
                .ok_or(ReplayError::Input("no Perfect baseline frame"))?;
        }
        for edge in &input.constraints {
            input.hit_frames[edge.after] =
                input.hit_frames[edge.after].max(input.hit_frames[edge.before] + edge.minimum_gap_frames);
        }
        if !perfect_baseline(&input) {
            return Err(ReplayError::Input(
                "could not construct a validated all-Perfect time-distance baseline",
            ));
        }
        Ok(input)
    }
}

fn perfect_baseline(input: &ReplayInput) -> bool {
    replay(input).is_ok_and(|r| r.trace.iter().all(|n| n.raw == Judgement::Perfect))
}

#[cfg(test)]
mod tests;
