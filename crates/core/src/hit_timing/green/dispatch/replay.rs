//! Replay supplied native callback schedules without splitting their packets.
//! This certifies reduction to score events, not that arbitrary callback/fact
//! sequences can be produced by Unity's device sampler or real-chart lifecycle.
//! The selected model allows one scoring settlement per continuous touch/frame.
//! Multi-score packets are rejected, never truncated or moved to later frames.
use super::super::lifetime::{LifetimeUpdate, UnsettledLifetime};
use super::{Callback, Chain, DispatchError, Facts, Node};
use crate::hit_timing::{replay, ReplayError, ReplayInput, ReplayResult};

pub enum Event<'a> {
    Hit {
        note: usize,
        frame: u32,
    },
    Green {
        chain: usize,
        frame: u32,
        facts: &'a [Facts],
        callback: Callback,
    },
    /// A real post-input native update of one post-head node. Preserve native
    /// node-update order. Missing updates must not be inferred from chart time.
    /// Ignored for already judged nodes; they no longer run unsettled StopState.
    AfterUpdate {
        chain: usize,
        node: usize,
        frame: u32,
        absolute_pos: i32,
        update: LifetimeUpdate,
    },
}
impl Event<'_> {
    fn frame(&self) -> u32 {
        match self {
            Self::Hit { frame, .. }
            | Self::Green { frame, .. }
            | Self::AfterUpdate { frame, .. } => *frame,
        }
    }
}

/// Each chain is paired with its head's scoring index. A head must have a prior
/// press in this schedule; post-head nodes can only be emitted by callbacks.
/// Raw grades must agree with the supplied frame domains; final correction and
/// scoring use the existing f32 replay and skill-queue semantics.
pub fn replay_callbacks(
    input: &ReplayInput,
    chains: &[(usize, &Chain)],
    events: &[Event<'_>],
) -> Result<ReplayResult, ReplayError> {
    let n = input.notes.len();
    let mut owner = vec![None; n];
    let mut heads = vec![None; n];
    for (ci, &(head, chain)) in chains.iter().enumerate() {
        if head >= n || owner[head].replace(ci).is_some() {
            return Err(ReplayError::Input(
                "green chains share or reference invalid notes",
            ));
        }
        heads[head] = Some(ci);
        for node in &chain.nodes {
            if let Node::Middle(i) | Node::PlainEnd(i) | Node::ArrowEnd(i) = *node {
                if i >= n || owner[i].replace(ci).is_some() {
                    return Err(ReplayError::Input(
                        "green chains share or reference invalid notes",
                    ));
                }
            }
        }
    }
    let mut cursors = vec![None; chains.len()];
    let mut last_scored_frame = vec![None; chains.len()];
    let mut judged: Vec<Vec<bool>> = chains.iter().map(|(_, c)| vec![false; c.len()]).collect();
    let mut lives: Vec<Vec<UnsettledLifetime>> = chains
        .iter()
        .map(|(_, c)| vec![UnsettledLifetime::default(); c.len()])
        .collect();
    let mut positions: Vec<Vec<Option<i32>>> =
        chains.iter().map(|(_, c)| vec![None; c.len()]).collect();
    let mut trial = input.clone();
    trial.hit_order.clear();
    trial.hit_frames = vec![0; n];
    let mut seen = vec![false; n];
    let mut last = 0;
    let mut after_update = false;
    for event in events {
        let frame = event.frame();
        if frame < last || frame > crate::hit_timing::MAX_SECONDS * crate::hit_timing::REPLAY_FPS {
            return Err(ReplayError::Input("invalid callback frame order"));
        }
        if frame != last {
            after_update = false;
        }
        if matches!(event, Event::AfterUpdate { .. }) {
            after_update = true;
        } else if after_update {
            return Err(ReplayError::Input(
                "input cannot follow the following update in the same frame",
            ));
        }
        last = frame;
        let hits = match *event {
            Event::Hit { note, .. } => {
                if note >= n || (owner[note].is_some() && heads[note].is_none()) {
                    return Err(ReplayError::Input(
                        "post-head green nodes require a callback",
                    ));
                }
                if let Some(c) = heads[note] {
                    cursors[c] = Some(0);
                }
                vec![(note, None)]
            }
            Event::Green {
                chain,
                facts,
                callback,
                ..
            } => {
                let Some(&(_, layout)) = chains.get(chain) else {
                    return Err(ReplayError::Input("invalid callback chain"));
                };
                let cursor =
                    cursors[chain].ok_or(ReplayError::Input("green callback precedes its head"))?;
                let packet = layout
                    .dispatch(cursor, facts, callback)
                    .map_err(|e| match e {
                        DispatchError::Input(s) => ReplayError::Input(s),
                        DispatchError::Miss(_) => {
                            ReplayError::Input("green callback causes unrepairable Miss")
                        }
                    })?;
                for &i in &packet.judged_hidden {
                    judged[chain][i] = true;
                }
                for settlement in &packet.settlements {
                    let i = layout
                        .nodes
                        .iter()
                        .position(|n| {
                            matches!(n,
                        Node::Middle(k) | Node::PlainEnd(k) | Node::ArrowEnd(k)
                        if *k == settlement.note)
                        })
                        .unwrap();
                    judged[chain][i] = true;
                }
                cursors[chain] = Some(packet.next_cursor);
                packet
                    .settlements
                    .into_iter()
                    .map(|s| (s.note, Some(s.raw)))
                    .collect()
            }
            Event::AfterUpdate {
                chain,
                node,
                absolute_pos,
                update,
                ..
            } => {
                let Some(&(_, layout)) = chains.get(chain) else {
                    return Err(ReplayError::Input("invalid update chain"));
                };
                if node >= layout.len()
                    || absolute_pos < 0
                    || matches!(update, LifetimeUpdate::HeadWait { .. })
                {
                    return Err(ReplayError::Input("invalid post-head update"));
                }
                let cursor =
                    cursors[chain].ok_or(ReplayError::Input("green update precedes its head"))?;
                if positions[chain][node].is_some_and(|p| p != absolute_pos) {
                    return Err(ReplayError::Input("green node absolute position changed"));
                }
                positions[chain][node] = Some(absolute_pos);
                if !judged[chain][node]
                    && lives[chain][node].update(absolute_pos, update)?.is_some()
                {
                    if layout.nodes[node] != Node::Hidden {
                        return Err(ReplayError::Input(
                            "visible green node expired before scoring",
                        ));
                    }
                    // onMiss tests the successor BEFORE SetJudge(Miss) of the
                    // expired hidden node. It does not test node == cursor.
                    let next = layout
                        .expire_hidden(cursor, node, judged[chain][node + 1])
                        .map_err(|_| ReplayError::Input("invalid hidden expiry callback"))?;
                    judged[chain][node] = true;
                    cursors[chain] = Some(next);
                }
                vec![]
            }
        };
        for (note, raw) in hits {
            if seen[note] {
                return Err(ReplayError::Input(
                    "callback schedule repeats a scoring note",
                ));
            }
            if let Some(chain) = owner[note] {
                if last_scored_frame[chain] == Some(frame) {
                    return Err(ReplayError::Input(
                        "same touch cannot settle multiple scoring notes in one frame",
                    ));
                }
                last_scored_frame[chain] = Some(frame);
            }
            if raw.is_some_and(|r| trial.notes[note].raw_judgement(frame).ok() != Some(r)) {
                return Err(ReplayError::Input(
                    "callback grade disagrees with declared frame domain",
                ));
            }
            seen[note] = true;
            trial.hit_frames[note] = frame;
            trial.hit_order.push(note);
        }
    }
    if seen.iter().any(|&s| !s) {
        return Err(ReplayError::Input("callback schedule omits scoring notes"));
    }
    for (i, &(_, chain)) in chains.iter().enumerate() {
        if cursors[i] != Some(chain.len()) {
            return Err(ReplayError::Input(
                "callback schedule leaves a green chain unfinished",
            ));
        }
    }
    replay(&trial)
}
