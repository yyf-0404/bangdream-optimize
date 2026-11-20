//! Conservative interval queries preserve the original note order. A note
//! omitted here has no frame inside the window, hence has an outside frame and
//! contributes no NoteOptions in the exhaustive builder. Nonempty domains are
//! guaranteed by FrameBounds, including notes requiring correction.
use super::super::replacement::frames::Frame;

pub(in crate::hit_timing::bounds) struct Index {
    spans: Vec<(u32, u32, usize)>,
    prefix_last: Vec<u32>,
}
impl Index {
    pub(in crate::hit_timing::bounds) fn storage_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.spans.capacity()*std::mem::size_of::<(u32,u32,usize)>()
            + self.prefix_last.capacity()*std::mem::size_of::<u32>()
    }
    pub(in crate::hit_timing::bounds) fn new(notes: &[Vec<Frame>]) -> Self {
        Self::from_spans(notes.iter().enumerate().map(|(i, n)| (
            n.iter().map(|f| f.time).min().unwrap(),
            n.iter().map(|f| f.time).max().unwrap(), i)).collect())
    }
    fn from_spans(mut spans: Vec<(u32, u32, usize)>) -> Self {
        spans.sort_unstable();
        let mut last = 0;
        let prefix_last = spans.iter().map(|s| { last = last.max(s.1); last }).collect();
        Self { spans, prefix_last }
    }
    pub(in crate::hit_timing::bounds) fn query(&self, first: u32, last: u32) -> Vec<usize> {
        let end = self.spans.partition_point(|s| s.0 <= last);
        let start = self.prefix_last[..end].partition_point(|&t| t < first);
        let mut ids: Vec<_> = self.spans[start..end].iter()
            .filter(|s| s.1 >= first).map(|s| s.2).collect();
        // Outward f64 additions are order dependent. Retain the exact order
        // used by the exhaustive builder, also for irregular/overlapping spans.
        ids.sort_unstable();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shuffled_nested_and_boundary_spans_match_exhaustive_intersections() {
        let spans = vec![(100,110,0),(4,400,1),(3,3,2),(100,100,3),(20,40,4),(7,8,5)];
        let index = Index::from_spans(spans.clone());
        for first in 0..=420 {
            for width in [0,1,7,18,100,500] {
                let last = first + width;
                let expected: Vec<_> = spans.iter().filter(|s| s.0 <= last && s.1 >= first)
                    .map(|s| s.2).collect();
                assert_eq!(index.query(first,last),expected,"interval {first}..{last}");
            }
        }
    }
}
