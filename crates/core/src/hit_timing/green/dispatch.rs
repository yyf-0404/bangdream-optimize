//! Post-head continuous-contact callback reduction for NoteSlide (CN 9.4.4).
//! Native-control-flow research oracle, not the timing-only optimizer action
//! model. Its gesture facts are not required as optimization search state.
//! One transition is one indivisible callback, not one independently timed node.
//! The caller supplies native gate/collision/lifetime facts for THAT SAME touch.
//! This does not generate those facts from Bestdori, model recovery after Miss,
//! or authorize arbitrary per-node hitboxes within a callback.
//! This reducer preserves native multi-settlement branches for comparison.
//! `replay_callbacks` enforces the selected one-score-per-touch/frame model;
//! native packets must not be truncated to manufacture a valid plan.
use crate::scoring::Judgement;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    Hidden,
    Middle(usize),
    PlainEnd(usize),
    ArrowEnd(usize),
}

/// Per-node observations when examined on THIS callback path, including prior
/// native Touch/state changes. They are not independently chosen frame facts.
/// `middle_ready` includes the Y[7] gate; raw Perfect alone is insufficient.
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    pub active: bool,
    pub middle_ready: bool,
    /// All entries describe the SAME touch position, not separate chosen points.
    pub inside: bool,
    pub release_raw: Judgement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    /// Includes Stationary. The boolean is the complete continuous arrow-end
    /// gate (position, direction/distance, leave counter), not merely a gesture.
    /// Evaluate it at the native tail stage, after any cacheTouchPos updates.
    Move {
        arrow_finish_passed: bool,
    },
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub note: usize,
    pub raw: Judgement,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    /// Next chain cursor; nodes.len() means the root has finished.
    pub next_cursor: usize,
    /// Must be replayed contiguously in this order at the callback's frame.
    pub settlements: Vec<Settlement>,
    /// Hidden nodes actually judged by the movement loop. Ended skips hidden
    /// nodes without judging them; cursor advancement alone is not this list.
    pub judged_hidden: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DispatchError {
    #[error("invalid green callback input: {0}")]
    Input(&'static str),
    /// This path necessarily emits Miss, which no supported skill repairs.
    /// Its recovery state is intentionally outside the P/Great optimization.
    #[error("green callback would miss node {0}")]
    Miss(usize),
}

/// The already-successful head is omitted. Intermediate hidden nodes survive
/// in this layout, but release jumps over a whole hidden run in constant time.
pub struct Chain {
    nodes: Vec<Node>,
    next_visible: Vec<usize>,
    reenter_middle: bool,
}
impl Chain {
    pub fn new(nodes: Vec<Node>, adjust_b: i32) -> Result<Self, DispatchError> {
        let Some(last) = nodes.last() else {
            return Err(DispatchError::Input("empty post-head chain"));
        };
        if !matches!(last, Node::PlainEnd(_) | Node::ArrowEnd(_)) {
            return Err(DispatchError::Input("chain needs one visible endpoint"));
        }
        let mut seen = HashSet::new();
        for (i, node) in nodes.iter().enumerate() {
            if i + 1 < nodes.len() && matches!(node, Node::PlainEnd(_) | Node::ArrowEnd(_)) {
                return Err(DispatchError::Input("endpoint before chain end"));
            }
            if let Node::Middle(n) | Node::PlainEnd(n) | Node::ArrowEnd(n) = node {
                if !seen.insert(*n) {
                    return Err(DispatchError::Input("repeated scoring note"));
                }
            }
        }
        let mut next_visible = vec![nodes.len(); nodes.len()];
        let mut next = nodes.len();
        for i in (0..nodes.len()).rev() {
            if nodes[i] != Node::Hidden {
                next = i;
            }
            next_visible[i] = next;
        }
        Ok(Self {
            nodes,
            next_visible,
            reenter_middle: adjust_b != 0,
        })
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// `cursor` follows the previous packet; only native post-head entry may
    /// initialize it. A no-score packet can still advance hidden nodes.
    /// An inactive node is not silently revived or assigned a guessed timeout.
    pub fn dispatch(
        &self,
        cursor: usize,
        facts: &[Facts],
        callback: Callback,
    ) -> Result<Packet, DispatchError> {
        if cursor >= self.nodes.len() || facts.len() != self.nodes.len() {
            return Err(DispatchError::Input(
                "cursor/facts do not describe an active chain",
            ));
        }
        let mut packet = Packet {
            next_cursor: cursor,
            settlements: vec![],
            judged_hidden: vec![],
        };
        if callback == Callback::Release {
            return self.release(packet, facts, false);
        }
        let Callback::Move {
            arrow_finish_passed,
        } = callback
        else {
            unreachable!()
        };
        if arrow_finish_passed
            && (!matches!(self.nodes.last(), Some(Node::ArrowEnd(_)))
                || facts.last().unwrap().release_raw != Judgement::Perfect)
        {
            return Err(DispatchError::Input(
                "continuous arrow gate conflicts with endpoint facts",
            ));
        }
        loop {
            let i = packet.next_cursor;
            match self.nodes[i] {
                Node::Hidden | Node::Middle(_) => {
                    if !facts[i].active {
                        return Err(DispatchError::Input(
                            "inactive current node needs lifecycle adaptation",
                        ));
                    }
                    if !facts[i].middle_ready || (self.nodes[i] != Node::Hidden && !facts[i].inside)
                    {
                        break;
                    }
                    if let Node::Middle(note) = self.nodes[i] {
                        packet.settlements.push(Settlement {
                            note,
                            raw: Judgement::Perfect,
                        });
                    } else {
                        packet.judged_hidden.push(i);
                    }
                    packet.next_cursor += 1;
                    // Original 0x1e8f010: B!=0 re-enters ExecTouchMoved (slot20).
                    // B==0 still reaches the arrow-end handling below.
                    if !self.reenter_middle {
                        break;
                    }
                }
                Node::PlainEnd(_) | Node::ArrowEnd(_) => break,
            }
        }
        if arrow_finish_passed {
            // Successful continuous arrow handling immediately invokes Ended;
            // another finger's event cannot be inserted before the release.
            self.release(packet, facts, true)
        } else {
            Ok(packet)
        }
    }

    /// Temporal onMissAfterNote callback for an INVISIBLE node. Unlike a
    /// visible expiry it emits no scoring Miss. Native onMiss 0x1e9239c checks
    /// only the direct successor's Judge == -1, not target == current.
    /// The caller supplies an actual expiry occurrence and the successor judge
    /// observed BEFORE this update, then marks the target judged (Miss).
    /// This is a cursor reduction, not a proof the expiry can occur on a chart.
    pub fn expire_hidden(
        &self,
        cursor: usize,
        target: usize,
        successor_judged: bool,
    ) -> Result<usize, DispatchError> {
        if cursor >= self.len() || self.nodes.get(target) != Some(&Node::Hidden) {
            return Err(DispatchError::Input("invalid hidden expiry callback"));
        }
        // Layout validation guarantees a direct successor for every hidden node.
        Ok(if successor_judged { cursor } else { target + 1 })
    }

    fn release(
        &self,
        mut packet: Packet,
        facts: &[Facts],
        arrow_ready: bool,
    ) -> Result<Packet, DispatchError> {
        // Ended skips invisible current nodes WITHOUT waiting for their Y gate.
        let i = self.next_visible[packet.next_cursor];
        if !facts[i].active {
            return Err(DispatchError::Input(
                "inactive release target needs lifecycle adaptation",
            ));
        }
        let (note, raw) = match self.nodes[i] {
            Node::PlainEnd(note) if facts[i].inside && facts[i].release_raw >= Judgement::Bad => {
                (note, facts[i].release_raw)
            }
            Node::ArrowEnd(note) if arrow_ready => (note, Judgement::Perfect),
            Node::Hidden => unreachable!("validated visible endpoint terminates every run"),
            _ => return Err(DispatchError::Miss(i)),
        };
        packet.settlements.push(Settlement { note, raw });
        packet.next_cursor = self.nodes.len();
        Ok(packet)
    }
}

#[cfg(test)]
mod tests;

pub mod replay;

#[cfg(test)]
mod frame_policy_tests;

#[cfg(test)]
mod lifecycle_tests;
