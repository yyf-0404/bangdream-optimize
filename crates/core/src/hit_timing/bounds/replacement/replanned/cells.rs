//! A row changes only when a run enters/leaves the window or a possible
//! preceding-note count changes. Cuts include every run endpoint and therefore
//! also every whole-note endpoint. All notes keep their original addition order.
use super::*;

pub(in crate::hit_timing::bounds::replacement) const MAX_BYTES: usize = 512 * 1024;

pub(in crate::hit_timing::bounds::replacement) struct Index {
    profile_rows: Vec<usize>,
    rows: Vec<Option<Vec<u32>>>,
}

impl Index {
    pub(in crate::hit_timing::bounds::replacement) fn new(
        profiles: &[Profile], notes: &[Vec<frames::Frame>],
    ) -> Self {
        if profiles.len() > (MAX_BYTES-std::mem::size_of::<Self>())
            / (std::mem::size_of::<usize>()+std::mem::size_of::<Option<Vec<u32>>>()) {
            return Self { profile_rows: vec![], rows: vec![] };
        }
        let mut widths: Vec<_> = profiles.iter().map(|p| p.playing).collect();
        widths.sort_unstable(); widths.dedup();
        let profile_rows: Vec<_> = profiles.iter()
            .map(|p| widths.binary_search(&p.playing).unwrap()).collect();
        let mut left = Vec::new();
        let mut right = Vec::new();
        for note in notes {
            for (first,last) in runs::Runs::new(note).boundaries() {
                left.push(last); right.push(first);
            }
        }
        left.sort_unstable(); left.dedup();
        right.sort_unstable(); right.dedup();
        let mut index = Self { profile_rows, rows: Vec::with_capacity(widths.len()) };
        // Admit individual durations independently, retaining a bounded index.
        // An omitted duration executes the original uncanonicalized row.
        let mut bytes = std::mem::size_of::<Self>()
            + index.profile_rows.capacity()*std::mem::size_of::<usize>()
            + index.rows.capacity()*std::mem::size_of::<Option<Vec<u32>>>();
        for width in widths {
            if left.len().saturating_add(right.len()).saturating_add(1)
                .saturating_mul(std::mem::size_of::<u32>()) > 2*MAX_BYTES {
                index.rows.push(None); continue;
            }
            let mut cuts = Vec::with_capacity(left.len()+right.len()+1);
            cuts.push(0);
            cuts.extend_from_slice(&left);
            cuts.extend(right.iter().filter_map(|&first| first.checked_sub(width)));
            cuts.sort_unstable(); cuts.dedup(); cuts.shrink_to_fit();
            let retained = cuts.capacity()*std::mem::size_of::<u32>();
            if bytes.saturating_add(retained) > MAX_BYTES {
                index.rows.push(None);
            } else {
                bytes += retained; index.rows.push(Some(cuts));
            }
        }
        debug_assert!(index.bytes() <= MAX_BYTES);
        index
    }

    pub(in crate::hit_timing::bounds::replacement) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.profile_rows.capacity()*std::mem::size_of::<usize>()
            + self.rows.capacity()*std::mem::size_of::<Option<Vec<u32>>>()
            + self.rows.iter().flatten().map(|r| r.capacity()*std::mem::size_of::<u32>()).sum::<usize>()
    }

    pub(super) fn start(&self, id: usize, width: u32, start: u32) -> u32 {
        if start.checked_add(width).is_none() { return start; }
        let Some(cuts) = self.profile_rows.get(id).and_then(|&i| self.rows[i].as_ref())
            else { return start; };
        cuts[cuts.partition_point(|&cut| cut <= start)-1]
    }
}

/// The exact witness row observes only the first/last included note indices.
/// Their most recent membership change is a canonical actual start frame;
/// using that frame as the key remains compatible with original raw-row keys.
pub(super) fn witness_start(notes: &[(u32,f64)], first: usize, last: usize,
    width: u32, start: u32) -> u32 {
    if start.checked_add(width).is_none() { return start; }
    let leaving = if first == 0 { 0 } else { notes[first-1].0 };
    let entering = if last == 0 { 0 } else { notes[last-1].0.saturating_sub(width) };
    leaving.max(entering)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duration_admission_remains_bounded_and_missing_rows_use_actual_frames() {
        let profiles:Vec<_>=(1..=30).map(|width| Profile::new(width,0,vec![[1.;4]])).collect();
        let frames:Vec<_>=(0..9000).map(|time| frames::Frame {
            time,raw:if time%2==0 {Judgement::Great} else {Judgement::Perfect},
            weight:[0.001,0.002] }).collect();
        let index=Index::new(&profiles,&[frames]);
        assert!(index.bytes()<=MAX_BYTES);
        assert!(index.rows.iter().any(Option::is_some));
        assert!(index.rows.iter().any(Option::is_none));
        for (id,&row) in index.profile_rows.iter().enumerate() {
            if index.rows[row].is_none() {assert_eq!(index.start(id,profiles[id].playing,100),100);}
        }
    }
}
