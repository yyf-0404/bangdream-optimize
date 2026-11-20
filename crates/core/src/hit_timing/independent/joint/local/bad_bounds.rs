//! Exact raw Bad bounds. Routing lifetime and causal constraints are separate.
use crate::hit_timing::{NoteWindow, ReplayError, ReplayNote, MAX_SECONDS, REPLAY_FPS};
use crate::scoring::Judgement;

pub(crate) struct BadBounds {
    pub bounds: Vec<(u32, u32)>,
    // Ordered by the left bound, not note time: explicit domains may be nested
    // and need not have monotonically increasing right bounds.
    order: Vec<usize>,
    pub judgement_probes: u64,
    pub same_time_reuses: u64,
}

#[derive(Clone, Copy)]
struct Cursor {
    time: f64,
    first: u32,
    last: u32,
}

impl BadBounds {
    pub fn new(notes: &[ReplayNote]) -> Result<Self, ReplayError> {
        let mut order: Vec<_> = (0..notes.len()).collect();
        if notes.windows(2).any(|w| w[0].time > w[1].time) {
            order.sort_unstable_by(|&a, &b| notes[a].time.total_cmp(&notes[b].time));
        }
        let mut result = Self {
            bounds: vec![(0, 0); notes.len()],
            order,
            judgement_probes: 0,
            same_time_reuses: 0,
        };
        // Width changes can move an endpoint backwards; never share a cursor
        // between Standard and WidenedEndpoint.
        let mut cursors: [Option<Cursor>; 2] = [None, None];
        for &index in &result.order {
            let note = &notes[index];
            let (coarse_first, coarse_last) = note.frame_search_bounds()?;
            let kind = match note.window {
                NoteWindow::Standard => 0,
                NoteWindow::WidenedEndpoint => 1,
                NoteWindow::SlideTime { .. } => {
                    result.bounds[index] = (coarse_first, coarse_last);
                    continue;
                }
                NoteWindow::NativeGreen {
                    first_frames,
                    last_frame,
                } => {
                    result.bounds[index] = (first_frames[0], last_frame);
                    continue;
                }
                NoteWindow::PinnedPerfect { frame } => {
                    result.bounds[index] = (frame, frame);
                    continue;
                }
            };
            if let Some(previous) = cursors[kind].filter(|c| c.time == note.time) {
                result.bounds[index] = (previous.first, previous.last);
                result.same_time_reuses += 1;
                continue;
            }
            // For one width, both exact Bad endpoints are nondecreasing in
            // note time. Subtraction, positive f32 conversion/division and
            // ties-to-even rounding preserve order on either side of the note.
            let mut first = cursors[kind].map_or(coarse_first, |c| c.first.max(coarse_first));
            while first <= coarse_last {
                result.judgement_probes += 1;
                if note.raw_judgement(first)? >= Judgement::Bad {
                    break;
                }
                first += 1;
            }
            if first > coarse_last {
                return Err(ReplayError::Input("empty raw Bad frame domain"));
            }
            // The floor sample lies within one 120-FPS frame of the note and
            // is always inside either Bad window. Jump across the interior,
            // then test the right boundary with the authoritative predicate.
            let centre = (note.time * f64::from(REPLAY_FPS))
                .floor()
                .clamp(0., f64::from(MAX_SECONDS * REPLAY_FPS)) as u32;
            let mut last = cursors[kind]
                .map_or(first, |c| c.last.max(first))
                .max(centre);
            while last < coarse_last {
                result.judgement_probes += 1;
                if note.raw_judgement(last + 1)? < Judgement::Bad {
                    break;
                }
                last += 1;
            }
            result.bounds[index] = (first, last);
            cursors[kind] = Some(Cursor {
                time: note.time,
                first,
                last,
            });
        }
        if result
            .order
            .windows(2)
            .any(|w| result.bounds[w[0]].0 > result.bounds[w[1]].0)
        {
            result.order.sort_unstable_by_key(|&i| result.bounds[i].0);
        }
        Ok(result)
    }

    pub fn payload_bytes(&self) -> usize {
        self.bounds.capacity() * std::mem::size_of::<(u32, u32)>()
            + self.order.capacity() * std::mem::size_of::<usize>()
    }

    /// Optional acceptance oracle; never used by the normal preparation path.
    pub fn audit(&self, notes: &[ReplayNote]) -> Result<(), ReplayError> {
        for (note, &(first, last)) in notes.iter().zip(&self.bounds) {
            let frames = note.frame_domain(Judgement::Bad)?;
            if frames.first().copied() != Some(first)
                || frames.last().copied() != Some(last)
                || frames.len() != (last - first + 1) as usize
            {
                return Err(ReplayError::Input(
                    "Bad bounds differ from exhaustive f32 domain",
                ));
            }
        }
        Ok(())
    }

    /// Ranges are sorted and merged. Only their right boundary advances the
    /// pointer; consuming a range after an overlap would miss nested domains.
    pub fn protected(&self, notes: &[ReplayNote], ranges: &[(u32, u32)]) -> Vec<bool> {
        debug_assert_eq!(notes.len(), self.bounds.len());
        debug_assert!(ranges.windows(2).all(|w| w[0].1 < w[1].0));
        let mut protected = vec![false; notes.len()];
        let mut region = 0;
        for &index in &self.order {
            let (first, last) = self.bounds[index];
            while region < ranges.len() && ranges[region].1 < first {
                region += 1;
            }
            protected[index] = notes[index].trigger.is_some()
                || (region < ranges.len() && ranges[region].0 <= last);
        }
        protected
    }
}

#[cfg(test)]
mod tests;
