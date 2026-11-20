//! Native batch scheduling, before geometry/grade reduction. CN 9.4.4, B=0,
//! fixed 120 FPS. Inputs retain original BMS fractions, empty batches and order;
//! this module never reconstructs batches by rounding chart seconds.
use super::{
    activation::MovementState,
    clock::{MusicClock120, MUSIC_BAR_DIVISION_COUNT},
};
use crate::hit_timing::{ReplayError, MAX_SECONDS, REPLAY_FPS};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativePosition {
    pub bar: i32,
    pub numerator: i32,
    pub denominator: i32,
}
impl NativePosition {
    /// NoteBatchInformation::.ctor: integer multiply/divide, then integer bar add.
    pub fn absolute(self) -> Result<i32, ReplayError> {
        if self.bar < 0
            || self.numerator < 0
            || self.denominator <= 0
            || self.numerator >= self.denominator
        {
            return Err(ReplayError::Input("invalid native batch fraction"));
        }
        let fraction = self
            .numerator
            .checked_mul(MUSIC_BAR_DIVISION_COUNT)
            .ok_or(ReplayError::Input("native batch fraction overflow"))?
            / self.denominator;
        self.bar
            .checked_mul(MUSIC_BAR_DIVISION_COUNT)
            .and_then(|n| n.checked_add(fraction))
            .ok_or(ReplayError::Input("native batch position overflow"))
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BpmCommand {
    pub position: NativePosition,
    pub bpm: f32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeBatch {
    pub position: NativePosition,
    /// True only if the FINAL native informationList is empty. It still occupies
    /// a batch-index step, but does not wait for its position.
    #[serde(default)]
    pub empty: bool,
    /// First BPM command in native list order. Multiple commands in one batch
    /// need resolving at the source; do not take the last command arbitrarily.
    pub bpm: Option<BpmCommand>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BatchDisposition {
    Launched,
    Empty,
    BarZero,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchEvent {
    pub index: usize,
    pub absolute_position: i32,
    pub disposition: BatchDisposition,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchStep {
    /// Input sees this completed update on the NEXT input frame. Frame zero is
    /// the supplied fresh state before the first normal input/update pair.
    pub next_input_frame: u32,
    /// Existing notes run AFTER active BPM commands but BEFORE a new batch.
    pub existing_notes_clock: MusicClock120,
    pub after_launch_clock: MusicClock120,
    pub batch: Option<BatchEvent>,
    pub applied_bpm_batches: Vec<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchGate {
    Waiting,
    Ready,
    Stale,
}
/// Original IsNoteActivateJustNow rejects a past bar OR a position already at
/// MusicPos (and logs an error). Equality at LauncherPos, however, is allowed.
pub fn launch_gate(
    position: NativePosition,
    clock: MusicClock120,
) -> Result<LaunchGate, ReplayError> {
    let absolute = position.absolute()? as f32;
    let music = clock.music.absolute_position()?;
    let launcher = clock.launcher.absolute_position()?;
    Ok(
        if position.bar < clock.music.bar || absolute - music <= 0. {
            LaunchGate::Stale
        } else if absolute <= launcher {
            LaunchGate::Ready
        } else {
            LaunchGate::Waiting
        },
    )
}

/// The native active BPM loop increments its index even when updateBpm removes
/// the current element. Preserve that skip; a drain/filter loop changes behavior.
#[derive(Clone, Debug)]
pub struct LaunchTimeline {
    clock: MusicClock120,
    batches: Vec<NativeBatch>,
    cursor: usize,
    active_bpm: Vec<usize>,
    frame: u32,
}
impl LaunchTimeline {
    pub fn new(speed: f32, basic_bpm: f32, batches: Vec<NativeBatch>) -> Result<Self, ReplayError> {
        let mut previous = None;
        for batch in &batches {
            let pos = batch.position.absolute()?;
            if previous.is_some_and(|p| p >= pos) {
                return Err(ReplayError::Input(
                    "native batches must already be grouped and ordered",
                ));
            }
            previous = Some(pos);
            if let Some(command) = batch.bpm {
                if batch.empty
                    || !command.bpm.is_finite()
                    || command.bpm <= 0.
                    || command.position.absolute()? != pos
                {
                    return Err(ReplayError::Input("invalid native batch BPM command"));
                }
            }
        }
        Ok(Self {
            clock: MusicClock120::new(speed, basic_bpm)?,
            batches,
            cursor: 0,
            active_bpm: vec![],
            frame: 0,
        })
    }
    pub fn clock(&self) -> MusicClock120 {
        self.clock
    }
    pub fn all_batches_consumed(&self) -> bool {
        self.cursor == self.batches.len()
    }
    pub fn has_pending_bpm(&self) -> bool {
        !self.active_bpm.is_empty()
    }
    /// Advance one native update, with the verified existing-note/new-batch phase
    /// split returned explicitly. No frame skipping, while-due loop or search cap.
    pub fn update_120(&mut self) -> Result<LaunchStep, ReplayError> {
        if self.frame >= MAX_SECONDS * REPLAY_FPS {
            return Err(ReplayError::Input(
                "native launch trace exceeds supported chart duration",
            ));
        }
        let mut clock = self.clock;
        clock.update_120()?;
        let mut active = self.active_bpm.clone();
        let mut applied = vec![];
        let mut i = 0;
        while i < active.len() {
            let index = active[i];
            let command = self.batches[index].bpm.unwrap();
            let p = command.position;
            if clock.music.reached(p.bar, p.numerator, p.denominator)? {
                clock.apply_bpm(command.bpm)?;
                active.remove(i);
                applied.push(index);
            }
            i += 1;
        }
        let existing_notes_clock = clock;
        let mut event = None;
        if let Some(batch) = self.batches.get(self.cursor) {
            let disposition = if batch.empty {
                Some(BatchDisposition::Empty)
            } else if batch.position.bar == 0 {
                Some(BatchDisposition::BarZero)
            } else {
                match launch_gate(batch.position, clock)? {
                    LaunchGate::Waiting => None,
                    LaunchGate::Ready => Some(BatchDisposition::Launched),
                    LaunchGate::Stale => {
                        return Err(ReplayError::Input(
                            "native batch became stale before launch",
                        ))
                    }
                }
            };
            if let Some(disposition) = disposition {
                if disposition == BatchDisposition::Launched {
                    if let Some(command) = batch.bpm {
                        clock.launch_bpm(command.bpm)?;
                        active.push(self.cursor);
                    }
                }
                event = Some(BatchEvent {
                    index: self.cursor,
                    absolute_position: batch.position.absolute()?,
                    disposition,
                });
            }
        }
        self.clock = clock;
        self.active_bpm = active;
        self.frame += 1;
        if event.is_some() {
            self.cursor += 1;
        }
        Ok(LaunchStep {
            next_input_frame: self.frame,
            existing_notes_clock,
            after_launch_clock: clock,
            batch: event,
            applied_bpm_batches: applied,
        })
    }
}

impl LaunchStep {
    /// A ROOT note activated by this batch. Later nodes already exist in Wait
    /// and must instead use MovementState::wait_120 on existing-note updates.
    pub fn root_seed(
        &self,
        absolute_pos: i32,
        note_bpm: f32,
        speed: f32,
    ) -> Result<MovementState, ReplayError> {
        if !self.batch.is_some_and(|b| {
            b.disposition == BatchDisposition::Launched && b.absolute_position == absolute_pos
        }) {
            return Err(ReplayError::Input(
                "root must belong to the native batch launched in this update",
            ));
        }
        let mut state = MovementState::default();
        state.activate(
            self.after_launch_clock.launcher.absolute_position()?,
            absolute_pos,
            note_bpm,
            speed,
        )?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pos(tick: i32) -> NativePosition {
        NativePosition {
            bar: tick / 192,
            numerator: tick % 192,
            denominator: 192,
        }
    }
    fn batch(tick: i32, bpm: Option<f32>) -> NativeBatch {
        NativeBatch {
            position: pos(tick),
            empty: false,
            bpm: bpm.map(|bpm| BpmCommand {
                position: pos(tick),
                bpm,
            }),
        }
    }
    #[test]
    fn native_fraction_truncates_before_bar_addition() {
        assert_eq!(
            NativePosition {
                bar: 2,
                numerator: 1,
                denominator: 7
            }
            .absolute()
            .unwrap(),
            411
        );
        assert!(NativePosition {
            bar: 2,
            numerator: 7,
            denominator: 7
        }
        .absolute()
        .is_err());
    }
    #[test]
    fn dense_batches_launch_on_separate_updates_and_seed_correct_input_frame() {
        let mut t = LaunchTimeline::new(
            1.,
            120.,
            vec![batch(192, None), batch(193, None), batch(194, None)],
        )
        .unwrap();
        for expected in 0..3 {
            let s = t.update_120().unwrap();
            assert_eq!(s.batch.unwrap().index, expected);
            assert_eq!(s.next_input_frame, expected as u32 + 1);
            assert!(
                s.root_seed(192 + expected as i32, 120., 1.)
                    .unwrap()
                    .progress
                    > 0.
            );
        }
        assert!(t.all_batches_consumed());
    }
    #[test]
    fn root_seed_rejects_another_batch_or_an_update_without_launch() {
        let mut t = LaunchTimeline::new(1., 120., vec![batch(192, None)]).unwrap();
        let launch = t.update_120().unwrap();
        assert!(launch.root_seed(193, 120., 1.).is_err());
        assert!(launch.root_seed(192, 120., 1.).is_ok());
        let following = t.update_120().unwrap();
        assert!(following.root_seed(192, 120., 1.).is_err());
    }
    #[test]
    fn empty_and_bar_zero_each_consume_exactly_one_update() {
        let mut empty = batch(193, None);
        empty.empty = true;
        let mut t =
            LaunchTimeline::new(12., 120., vec![batch(0, None), empty, batch(194, None)]).unwrap();
        assert_eq!(
            t.update_120().unwrap().batch.unwrap().disposition,
            BatchDisposition::BarZero
        );
        assert_eq!(
            t.update_120().unwrap().batch.unwrap().disposition,
            BatchDisposition::Empty
        );
        assert!(t.update_120().unwrap().batch.is_none());
    }
    #[test]
    fn launch_equality_is_allowed_but_music_equality_is_stale() {
        let mut c = MusicClock120::new(10., 120.).unwrap();
        c.launcher = super::super::clock::BarProgress { bar: 1, beat: 0. };
        assert_eq!(launch_gate(pos(192), c).unwrap(), LaunchGate::Ready);
        assert_eq!(launch_gate(pos(193), c).unwrap(), LaunchGate::Waiting);
        c.music = c.launcher;
        assert_eq!(launch_gate(pos(192), c).unwrap(), LaunchGate::Stale);
    }
    #[test]
    fn bpm_launch_and_effect_are_separated_without_double_move() {
        let mut t = LaunchTimeline::new(10., 120., vec![batch(192, Some(240.))]).unwrap();
        let launch = loop {
            let s = t.update_120().unwrap();
            if s.batch.is_some() {
                break s;
            }
        };
        assert_eq!(launch.existing_notes_clock.launcher_bpm, 120.);
        assert_eq!(launch.after_launch_clock.launcher_bpm, 240.);
        assert_eq!(launch.after_launch_clock.current_bpm, 120.);
        assert!(
            launch.root_seed(192, 240., 10.).unwrap().real_move_seconds
                < super::super::clock::FRAME_SECONDS * 2.
        );
        let change = loop {
            let s = t.update_120().unwrap();
            if !s.applied_bpm_batches.is_empty() {
                break s;
            }
        };
        assert_eq!(change.existing_notes_clock.current_bpm, 240.);
        assert!(change.next_input_frame > launch.next_input_frame);
        assert!(!t.has_pending_bpm());
    }
    #[test]
    fn native_removal_skips_shifted_next_bpm_until_following_update() {
        let mut t = LaunchTimeline::new(
            1.,
            1000.,
            vec![batch(192, Some(1100.)), batch(193, Some(1200.))],
        )
        .unwrap();
        t.update_120().unwrap();
        t.update_120().unwrap();
        t.clock.music = super::super::clock::BarProgress { bar: 1, beat: 20. };
        assert_eq!(t.update_120().unwrap().applied_bpm_batches, vec![0]);
        assert_eq!(t.update_120().unwrap().applied_bpm_batches, vec![1]);
    }
    #[test]
    fn stale_batches_fail_atomically_instead_of_skipping_or_hanging() {
        let mut t = LaunchTimeline::new(1., 120., vec![batch(192, None)]).unwrap();
        t.clock.music = super::super::clock::BarProgress { bar: 1, beat: 0. };
        let bits = t.clock.music.beat.to_bits();
        assert!(t.update_120().is_err());
        assert_eq!(t.frame, 0);
        assert_eq!(t.cursor, 0);
        assert_eq!(t.clock.music.beat.to_bits(), bits);
        assert!(LaunchTimeline::new(1., 120., vec![batch(192, None), batch(192, None)]).is_err());
    }
}
