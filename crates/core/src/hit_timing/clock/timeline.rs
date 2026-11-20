//! Shared input-frame view of the native FIFO skill clock. Constructing a
//! schedule does not require ordinary hits to be legal at their old positions.
use super::*;
use crate::hit_timing::{routing::event_ranks, NoteWindow, ReplayInput, MAX_SECONDS, REPLAY_FPS};

pub(crate) type TimelineKey = Vec<(u32, usize, usize)>;

pub(crate) struct Timeline {
    active: Vec<Option<usize>>,
}

impl Timeline {
    pub fn key(input: &ReplayInput, frames: &[u32]) -> Result<TimelineKey, ReplayError> {
        Self::key_with_touch_order(input, frames, false)
    }

    pub fn key_with_touch_order(
        input: &ReplayInput,
        frames: &[u32],
        optimize: bool,
    ) -> Result<TimelineKey, ReplayError> {
        if frames.len() != input.notes.len() || frames.iter().any(|&f| f > MAX_SECONDS * REPLAY_FPS)
        {
            return Err(ReplayError::Input("invalid candidate frame count or range"));
        }
        for (index, (note, &frame)) in input.notes.iter().zip(frames).enumerate() {
            if matches!(note.window, NoteWindow::PinnedPerfect { frame: pinned } if pinned != frame)
            {
                return Err(ReplayError::Note {
                    index,
                    reason: "pinned node moved",
                });
            }
        }
        let ranks = event_ranks(input);
        let mut key: TimelineKey = input
            .notes
            .iter()
            .enumerate()
            .filter_map(|(i, note)| note.trigger.map(|slot| (frames[i], i, slot)))
            .collect();
        key.sort_unstable_by_key(|&(frame, i, slot)| {
            (frame, if optimize { slot } else { ranks[i] })
        });
        if key.windows(2).any(|p| p[0].2 >= p[1].2) {
            return Err(ReplayError::SkillOrder);
        }
        Ok(key)
    }

    pub fn from_requests(skills: &[ReplaySkill], key: &TimelineKey) -> Result<Self, ReplayError> {
        let last = key.last().map_or(0, |r| r.0);
        let tail = (skills
            .iter()
            .map(|s| f64::from(s.duration) + 1.)
            .sum::<f64>()
            * f64::from(REPLAY_FPS))
        .ceil() as u32
            + 10;
        let mut clock = SkillClock::new(REPLAY_FPS);
        let mut cursor = 0;
        let mut active = Vec::new();
        for frame in 0..=last + tail {
            active.push(clock.active_index());
            while cursor < key.len() && key[cursor].0 == frame {
                clock.request(key[cursor].2, frame);
                cursor += 1;
            }
            clock.after_hits(frame, skills)?;
            if cursor == key.len() && clock.finished() {
                return Ok(Self { active });
            }
        }
        Err(ReplayError::ClockLimit)
    }

    pub fn slot(&self, frame: u32) -> Option<usize> {
        self.active.get(frame as usize).copied().flatten()
    }
}
