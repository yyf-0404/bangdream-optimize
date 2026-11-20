use super::{skill::SkillState, ReplayError, ReplaySkill, SkillEvent};
use std::collections::VecDeque;

mod timeline;
pub(super) use timeline::Timeline;

#[derive(PartialEq)]
enum Phase {
    Idle,
    Pending,
    Playing,
    Cooling,
}

pub(super) struct SkillClock {
    phase: Phase,
    delta: f32,
    remaining: f32,
    queued: VecDeque<usize>,
    active: Option<(usize, SkillState)>,
    pub events: Vec<SkillEvent>,
}

impl SkillClock {
    pub fn new(fps: u32) -> Self {
        Self {
            phase: Phase::Idle,
            delta: 1.0_f32 / fps as f32,
            remaining: 0.0,
            queued: VecDeque::new(),
            active: None,
            events: Vec::new(),
        }
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active.as_ref().map(|(i, _)| *i)
    }
    pub fn active_mut(&mut self) -> Option<&mut SkillState> {
        self.active.as_mut().map(|(_, s)| s)
    }

    pub fn request(&mut self, index: usize, frame: u32) {
        self.queued.push_back(index);
        self.events.push(SkillEvent::Request { index, frame });
        if self.phase == Phase::Idle {
            self.phase = Phase::Pending;
        }
    }

    pub fn after_hits(&mut self, frame: u32, skills: &[ReplaySkill]) -> Result<(), ReplayError> {
        match self.phase {
            Phase::Pending => {
                let index = self
                    .queued
                    .pop_front()
                    .expect("pending transition requires a request");
                self.active = Some((index, SkillState::new(&skills[index])?));
                self.remaining = skills[index].duration;
                self.phase = Phase::Playing;
                self.events.push(SkillEvent::Start {
                    index,
                    after_frame: frame,
                    first_input_frame: frame + 1,
                });
            }
            Phase::Playing | Phase::Cooling => {
                // Check BEFORE decrement; perform at most one transition per
                // update. Do not replace by ceil(duration * fps) or +one frame.
                if self.remaining > 0.0 {
                    self.remaining -= self.delta;
                } else if self.phase == Phase::Playing {
                    let (index, _) = self
                        .active
                        .take()
                        .expect("playing requires an active skill");
                    self.events.push(SkillEvent::End {
                        index,
                        after_frame: frame,
                        last_input_frame: frame,
                    });
                    self.phase = Phase::Cooling;
                    self.remaining = 0.75_f32;
                } else {
                    self.phase = if self.queued.is_empty() {
                        Phase::Idle
                    } else {
                        Phase::Pending
                    };
                }
            }
            Phase::Idle => {}
        }
        Ok(())
    }

    pub fn finished(&self) -> bool {
        self.active.is_none() && self.queued.is_empty() && self.phase != Phase::Pending
    }
}
