//! Inline events need only 24 retained bytes. Rare runs retain their owned hits.
use super::super::{Action, ActionHits, Factor};
use std::ops::{Deref, Range};

pub(in crate::hit_timing::independent) enum Actions {
    Wide(Vec<Action>),
    Compact { events: Box<[SmallAction]>, runs: Box<[Action]> },
}
pub(in crate::hit_timing::independent) struct SmallAction {
    reference: f64,
    rank: u32,
    note: u32,
    frame: u32,
    flags: u8,
    slot: u8,
}
const RUN: u8 = 8;
pub(in crate::hit_timing::independent) enum ActionRef<'a> { Borrowed(&'a Action), Event(Action) }
impl Deref for ActionRef<'_> {
    type Target = Action;
    fn deref(&self) -> &Action {
        match self { Self::Borrowed(action) => action, Self::Event(action) => action }
    }
}
impl From<Vec<Action>> for Actions {
    fn from(actions: Vec<Action>) -> Self { Self::Wide(actions) }
}
impl Actions {
    pub fn len(&self) -> usize {
        match self { Self::Wide(actions) => actions.len(), Self::Compact { events, .. } => events.len() }
    }
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        match self { Self::Wide(actions) => actions.capacity(), Self::Compact { events, .. } => events.len() }
    }
    pub fn get(&self, index: usize) -> ActionRef<'_> {
        match self {
            Self::Wide(actions) => ActionRef::Borrowed(&actions[index]),
            Self::Compact { events, runs } => {
                let a = &events[index];
                if a.flags & RUN != 0 { return ActionRef::Borrowed(&runs[a.note as usize]); }
                ActionRef::Event(Action { factor: Factor {
                    reference: a.reference, rank: a.rank as usize, slot: a.slot,
                    perfect: a.flags & 1 != 0, inside: a.flags & 2 != 0, fever: a.flags & 4 != 0,
                }, hits: ActionHits::Event([(a.note as usize, a.frame)]) })
            }
        }
    }
    pub fn iter(&self) -> ActionIter<'_> { ActionIter { actions: self, range: 0..self.len() } }
    pub fn wide(&self) -> &[Action] {
        match self { Self::Wide(actions) => actions, _ => panic!("frozen actions cannot be constructed") }
    }
    pub fn wide_mut(&mut self) -> &mut Vec<Action> {
        match self { Self::Wide(actions) => actions, _ => panic!("frozen actions cannot be constructed") }
    }
    pub fn push(&mut self, action: Action) { self.wide_mut().push(action); }
    pub fn extend_from_slice(&mut self, actions: &[Action]) { self.wide_mut().extend_from_slice(actions); }
    pub(super) fn freeze(&mut self) {
        let Self::Wide(actions) = self else { return; };
        if actions.iter().any(|a| u32::try_from(a.factor.rank).is_err()
            || matches!(&a.hits, ActionHits::Event([(note, _)]) if u32::try_from(*note).is_err()))
            || actions.iter().filter(|a| matches!(a.hits, ActionHits::Run(_))).count() > u32::MAX as usize { return; }
        let mut events = Vec::with_capacity(actions.len());
        let mut runs = Vec::new();
        for action in std::mem::take(actions) {
            let factor = action.factor;
            let flags = u8::from(factor.perfect) | (u8::from(factor.inside) << 1) | (u8::from(factor.fever) << 2);
            let (note, frame, flags) = match &action.hits {
                ActionHits::Event([(note, frame)]) => (*note as u32, *frame, flags),
                ActionHits::Run(_) => {
                    let index = runs.len() as u32;
                    runs.push(action);
                    (index, 0, flags | RUN)
                }
            };
            events.push(SmallAction { reference: factor.reference, rank: factor.rank as u32,
                note, frame, flags, slot: factor.slot });
        }
        *self = Self::Compact { events: events.into_boxed_slice(), runs: runs.into_boxed_slice() };
    }
    pub fn bytes(&self) -> usize {
        let hits = |a: &Action| match &a.hits {
            ActionHits::Event(_) => 0,
            ActionHits::Run(hits) => hits.capacity() * std::mem::size_of::<(usize, u32)>(),
        };
        match self {
            Self::Wide(actions) => actions.capacity() * std::mem::size_of::<Action>()
                + actions.iter().map(hits).sum::<usize>(),
            Self::Compact { events, runs } => events.len() * std::mem::size_of::<SmallAction>()
                + runs.len() * std::mem::size_of::<Action>() + runs.iter().map(hits).sum::<usize>(),
        }
    }
}
pub(in crate::hit_timing::independent) struct ActionIter<'a> { actions: &'a Actions, range: Range<usize> }
impl<'a> Iterator for ActionIter<'a> {
    type Item = ActionRef<'a>;
    fn next(&mut self) -> Option<Self::Item> { self.range.next().map(|i| self.actions.get(i)) }
    fn size_hint(&self) -> (usize, Option<usize>) { self.range.size_hint() }
}
impl DoubleEndedIterator for ActionIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> { self.range.next_back().map(|i| self.actions.get(i)) }
}
impl ExactSizeIterator for ActionIter<'_> {}
impl<'a> IntoIterator for &'a Actions {
    type Item = ActionRef<'a>;
    type IntoIter = ActionIter<'a>;
    fn into_iter(self) -> Self::IntoIter { self.iter() }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn signature(a: &Action) -> (bool, bool, bool, u8, usize, u64, bool, Vec<(usize,u32)>) {
        let f = a.factor;
        (f.perfect, f.inside, f.fever, f.slot, f.rank, f.reference.to_bits(),
            matches!(a.hits, ActionHits::Run(_)), a.hits.to_vec())
    }
    #[test]
    fn frozen_events_and_owned_runs_preserve_every_operand_bit_and_order() {
        let mut originals = vec![];
        for flags in 0..8 { for bits in [0, 1, 0x8000000000000000, 0x3ff0000000000001, 0x7ff8000000001234] {
            originals.push(Action { factor: Factor { perfect: flags & 1 != 0,
                inside: flags & 2 != 0, fever: flags & 4 != 0, slot: 255,
                reference: f64::from_bits(bits), rank: u32::MAX as usize },
                hits: ActionHits::Event([(u32::MAX as usize, u32::MAX)]) });
        } }
        for hits in [vec![(8,20)], vec![(8,20),(3,20),(1,21)]] {
            originals.push(Action { factor: originals[0].factor, hits: hits.into() });
        }
        let expected: Vec<_> = originals.iter().map(signature).collect();
        let mut actions: Actions = originals.into();
        let before = actions.bytes();
        actions.freeze();
        assert!(matches!(actions, Actions::Compact { .. }));
        assert_eq!(std::mem::size_of::<SmallAction>(), 24);
        assert!(actions.bytes() < before);
        assert_eq!(actions.iter().map(|a| signature(&a)).collect::<Vec<_>>(), expected);
        actions.freeze();
        assert_eq!(actions.iter().rev().map(|a| signature(&a)).collect::<Vec<_>>(), expected.into_iter().rev().collect::<Vec<_>>());
    }
    #[test]
    fn unrepresentable_event_keeps_wide_indices_without_truncation() {
        if usize::BITS <= 32 { return; }
        for (rank,note) in [(usize::MAX,0),(1,usize::MAX)] {
            let action = Action { factor: Factor { perfect: true, inside: false,
                fever: false, slot: 0, reference: 1., rank }, hits: ActionHits::Event([(note,1)]) };
            let expected = signature(&action);
            let mut actions: Actions = vec![action].into();
            actions.freeze();
            assert!(matches!(actions, Actions::Wide(_)));
            assert_eq!(signature(&actions.get(0)), expected);
        }
    }
}
