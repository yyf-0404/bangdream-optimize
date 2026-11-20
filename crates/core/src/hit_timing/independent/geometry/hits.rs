//! Single-event actions dominate both fresh graphs and cached template copies.
//! Keep that one hit inline; a collapsed Perfect run keeps its ordered Vec.
use std::ops::{Deref, DerefMut};

type Hit = (usize, u32);

#[derive(Clone, Debug)]
pub(crate) enum ActionHits {
    Event([Hit; 1]),
    Run(Vec<Hit>),
}
impl ActionHits {
    pub(super) fn event(note: usize, frame: u32, inline: bool) -> Self {
        if inline {
            Self::Event([(note, frame)])
        } else {
            Self::Run(vec![(note, frame)])
        }
    }
    pub(super) fn extend_from_slice(&mut self, hits: &[Hit]) {
        if hits.is_empty() {
            return;
        }
        if let Self::Event([first]) = self {
            let mut run = Vec::with_capacity(1 + hits.len());
            run.push(*first);
            *self = Self::Run(run);
        }
        if let Self::Run(run) = self {
            run.extend_from_slice(hits);
        }
    }
}
impl Deref for ActionHits {
    type Target = [Hit];
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Event(hit) => hit,
            Self::Run(hits) => hits,
        }
    }
}
impl DerefMut for ActionHits {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Event(hit) => hit,
            Self::Run(hits) => hits,
        }
    }
}
impl<'a> IntoIterator for &'a ActionHits {
    type Item = &'a Hit;
    type IntoIter = std::slice::Iter<'a, Hit>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a> IntoIterator for &'a mut ActionHits {
    type Item = &'a mut Hit;
    type IntoIter = std::slice::IterMut<'a, Hit>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
impl From<Vec<Hit>> for ActionHits {
    fn from(hits: Vec<Hit>) -> Self {
        Self::Run(hits)
    }
}
impl FromIterator<Hit> for ActionHits {
    fn from_iter<T: IntoIterator<Item = Hit>>(iter: T) -> Self {
        Self::Run(iter.into_iter().collect())
    }
}
impl PartialEq for ActionHits {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl Eq for ActionHits {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_clones_are_inline_and_run_extension_keeps_order_and_ownership() {
        let single = ActionHits::event(2, 33, true);
        let mut cloned = single.clone();
        assert!(matches!(cloned, ActionHits::Event(_)));
        cloned[0].1 += 1;
        assert_eq!(&*single, &[(2, 33)]);
        cloned.extend_from_slice(&[]);
        assert!(matches!(cloned, ActionHits::Event(_)));
        cloned.extend_from_slice(&[(0, 35), (1, 35)]);
        assert!(matches!(cloned, ActionHits::Run(_)));
        assert_eq!(&*cloned, &[(2, 34), (0, 35), (1, 35)]);
        assert_eq!(single, ActionHits::event(2, 33, false));
    }
}
