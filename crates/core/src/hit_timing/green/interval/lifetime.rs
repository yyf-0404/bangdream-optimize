//! Speed-independent music-clock thresholds for fixed-BPM green lifetime.
use super::FrameRange;
use crate::hit_timing::{
    green::{
        clock::{BarProgress, MusicClock120, MUSIC_BAR_DIVISION_COUNT},
        lifetime::{NativeClock, MISS_SECOND_INTERVAL},
        trajectory::NativeGreenChain,
        GreenRole,
    },
    ReplayError, MAX_SECONDS, REPLAY_FPS,
};

/// `positions[f]` is the native music position after update `f`, which is the
/// clock used by trajectory.step(f). The launcher is initialized at zero only
/// to reuse MusicClock120's exact f32 update order; its value is never read.
pub struct FixedBpmMusicTrace {
    bpm: f32,
    positions: Vec<f32>,
}

fn later(a: FrameRange, b: FrameRange) -> FrameRange {
    FrameRange {
        lower: a.lower.max(b.lower),
        upper: a.upper.max(b.upper),
    }
}

fn earlier(a: FrameRange, b: FrameRange) -> FrameRange {
    FrameRange {
        lower: a.lower.min(b.lower),
        upper: a.upper.min(b.upper),
    }
}

fn at(frame: u32) -> FrameRange {
    FrameRange {
        lower: frame,
        upper: frame,
    }
}

fn hull(a: FrameRange, b: FrameRange) -> FrameRange {
    FrameRange {
        lower: a.lower.min(b.lower),
        upper: a.upper.max(b.upper),
    }
}

/// Enclose each node's unsettled expiry from already certified stop-frame
/// ranges. With B=0 and no BPM command, music thresholds do not depend on
/// speed. A midpoint test is nonmonotone once music reaches the successor;
/// a stop range spanning that boundary is split into both discrete branches
/// and their expiry bounds are hulled, never treated as permanently true.
pub fn constant_bpm_expiry_bounds(
    chain: &NativeGreenChain,
    stops: &[FrameRange],
    music: &FixedBpmMusicTrace,
) -> Result<Vec<FrameRange>, ReplayError> {
    if chain.nodes.len() < 2
        || chain.nodes.len() != stops.len()
        || chain.nodes[0].note_index.is_none()
        || chain.nodes.last().is_some_and(|n| n.note_index.is_none())
    {
        return Err(ReplayError::Input("invalid native expiry chain bounds"));
    }
    let mut results = Vec::with_capacity(stops.len());
    for (i, node) in chain.nodes.iter().enumerate() {
        let stop = stops[i];
        if stop.lower > stop.upper || stop.upper > music.last_update_frame() {
            return Err(ReplayError::Input("stop frame exceeds music proof"));
        }
        let next_visible =
            (i + 1..chain.nodes.len()).find(|&j| chain.nodes[j].note_index.is_some());
        let expiry = if i == 0 {
            let next = next_visible.ok_or(ReplayError::Input("head has no visible successor"))?;
            let threshold = music
                .first_head_wait_expiry(
                    node.absolute_position,
                    chain.nodes[next].absolute_position,
                )?
                .ok_or(ReplayError::Input("head expiry exceeds music proof"))?;
            later(stop, at(threshold))
        } else if i + 1 == chain.nodes.len() {
            match node.role {
                GreenRole::HeldArrowEnd => {
                    let add = |frame: u32| {
                        frame
                            .checked_add(13)
                            .filter(|&f| f <= music.last_update_frame())
                            .ok_or(ReplayError::Input("arrow wait exceeds music proof"))
                    };
                    FrameRange {
                        lower: add(stop.lower)?,
                        upper: add(stop.upper)?,
                    }
                }
                GreenRole::PlainEnd => {
                    let timeout = music
                        .first_timeout(node.absolute_position)?
                        .ok_or(ReplayError::Input("end timeout exceeds music proof"))?;
                    later(stop, at(timeout))
                }
                _ => return Err(ReplayError::Input("invalid native terminal role")),
            }
        } else {
            if !matches!(node.role, GreenRole::HeldMiddle) {
                return Err(ReplayError::Input("invalid native after-node role"));
            }
            let next = next_visible.ok_or(ReplayError::Input("missing visible after-node"))?;
            let next_stop = stops[next];
            if next_stop.lower > next_stop.upper || next_stop.upper > music.last_update_frame() {
                return Err(ReplayError::Input("next stop exceeds music proof"));
            }
            let timeout = music
                .first_timeout(node.absolute_position)?
                .ok_or(ReplayError::Input("after-node timeout exceeds music proof"))?;
            let without_midpoint =
                |s: FrameRange| earlier(later(s, next_stop), later(s, at(timeout)));
            if let Some((midpoint, end_exclusive)) = music.after_midpoint_window(
                node.absolute_position,
                chain.nodes[next].absolute_position,
            )? {
                let before = (stop.lower < end_exclusive).then(|| FrameRange {
                    lower: stop.lower,
                    upper: stop.upper.min(end_exclusive - 1),
                });
                let after = (stop.upper >= end_exclusive).then(|| FrameRange {
                    lower: stop.lower.max(end_exclusive),
                    upper: stop.upper,
                });
                match (before, after) {
                    (Some(before), Some(after)) => {
                        let with_midpoint =
                            earlier(without_midpoint(before), later(before, at(midpoint)));
                        hull(with_midpoint, without_midpoint(after))
                    }
                    (Some(before), None) => {
                        earlier(without_midpoint(before), later(before, at(midpoint)))
                    }
                    (None, Some(after)) => without_midpoint(after),
                    (None, None) => unreachable!("a valid stop interval must select a branch"),
                }
            } else {
                without_midpoint(stop)
            }
        };
        results.push(expiry);
    }
    Ok(results)
}

impl FixedBpmMusicTrace {
    pub fn new(bpm: f32, last_update_frame: u32) -> Result<Self, ReplayError> {
        if !bpm.is_finite() || bpm <= 0.0 || last_update_frame >= MAX_SECONDS * REPLAY_FPS {
            return Err(ReplayError::Input("invalid fixed-BPM music trace"));
        }
        let mut clock = MusicClock120 {
            music: BarProgress::default(),
            launcher: BarProgress::default(),
            current_bpm: bpm,
            launcher_bpm: bpm,
        };
        let mut positions = Vec::with_capacity(last_update_frame as usize + 1);
        for _ in 0..=last_update_frame {
            clock.update_120()?;
            let position = clock.music.absolute_position()?;
            if positions
                .last()
                .is_some_and(|&previous| position <= previous)
            {
                return Err(ReplayError::Input(
                    "native music clock is not strictly increasing",
                ));
            }
            positions.push(position);
        }
        Ok(Self { bpm, positions })
    }

    pub fn last_update_frame(&self) -> u32 {
        self.positions.len() as u32 - 1
    }

    pub fn position_after_update(&self, update_frame: u32) -> Option<f32> {
        self.positions.get(update_frame as usize).copied()
    }

    fn first_true(
        &self,
        predicate: impl Fn(f32) -> Result<bool, ReplayError>,
    ) -> Result<Option<u32>, ReplayError> {
        let mut low = 0usize;
        let mut high = self.positions.len();
        while low < high {
            let middle = low + (high - low) / 2;
            if predicate(self.positions[middle])? {
                high = middle;
            } else {
                low = middle + 1;
            }
        }
        Ok((low < self.positions.len()).then_some(low as u32))
    }

    fn late_seconds(&self, music: f32, tick: i32) -> Result<f32, ReplayError> {
        NativeClock {
            adjusted_music_pos: music,
            current_bpm: self.bpm,
            resolution: MUSIC_BAR_DIVISION_COUNT,
        }
        .late_seconds(tick)
    }

    /// First update for which the native `late > MISS_SECOND_INTERVAL` is true.
    /// `None` means the supplied trace horizon is insufficient.
    pub fn first_timeout(&self, tick: i32) -> Result<Option<u32>, ReplayError> {
        if tick < 0 {
            return Err(ReplayError::Input("invalid native green timeout tick"));
        }
        self.first_true(|music| Ok(self.late_seconds(music, tick)? > MISS_SECOND_INTERVAL))
    }

    /// First frame satisfying the HeadWait expiry test, independent of speed.
    pub fn first_head_wait_expiry(
        &self,
        tick: i32,
        next_visible_tick: i32,
    ) -> Result<Option<u32>, ReplayError> {
        if tick < 0 || next_visible_tick <= tick {
            return Err(ReplayError::Input("invalid native head expiry positions"));
        }
        self.first_true(|music| {
            let late = self.late_seconds(music, tick)?;
            let since = music - tick as f32;
            let until = next_visible_tick as f32 - music;
            Ok(late > MISS_SECOND_INTERVAL || until <= 0.0 || since > until)
        })
    }

    /// Consecutive update-frame interval [first, end_exclusive) in which the
    /// native AfterMidpoint branch's `until>0 && since>until` holds. It may be
    /// empty. `end_exclusive == last_update_frame+1` means the music position
    /// never reached the successor before this trace ended.
    pub fn after_midpoint_window(
        &self,
        tick: i32,
        next_visible_tick: i32,
    ) -> Result<Option<(u32, u32)>, ReplayError> {
        if tick < 0 || next_visible_tick <= tick {
            return Err(ReplayError::Input("invalid native midpoint positions"));
        }
        let first = self.first_true(|music| {
            let since = music - tick as f32;
            let until = next_visible_tick as f32 - music;
            Ok(since > until)
        })?;
        let end = self.first_true(|music| Ok(next_visible_tick as f32 - music <= 0.0))?;
        match first {
            None => Ok(None),
            Some(first) => {
                let exclusive = end.unwrap_or(self.last_update_frame() + 1);
                Ok((first < exclusive).then_some((first, exclusive)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::green::{
        lifetime::{LifetimeUpdate, UnsettledLifetime, VisibleAfter, VisibleState},
        trajectory::NativeGreenNode,
    };

    #[test]
    fn fixed_music_trace_matches_native_timeline_at_different_speeds() {
        let trace = FixedBpmMusicTrace::new(185.0, 700).unwrap();
        for speed in [1.0, 10.0, 12.0] {
            let mut clock = MusicClock120::new(speed, 185.0).unwrap();
            for frame in 0..=700 {
                clock.update_120().unwrap();
                assert_eq!(
                    trace.position_after_update(frame).unwrap().to_bits(),
                    clock.music.absolute_position().unwrap().to_bits()
                );
            }
        }
    }

    #[test]
    fn threshold_binary_search_matches_native_lifetime_updates() {
        let trace = FixedBpmMusicTrace::new(185.0, 1300).unwrap();
        let tick = 384;
        let next = 432;
        let timeout = trace.first_timeout(tick).unwrap().unwrap();
        let head = trace.first_head_wait_expiry(tick, next).unwrap().unwrap();
        let midpoint = trace.after_midpoint_window(tick, next).unwrap().unwrap();
        assert!(head <= timeout);
        assert!(midpoint.0 < midpoint.1);
        for frame in 0..=trace.last_update_frame() {
            let music = trace.position_after_update(frame).unwrap();
            let clock = NativeClock {
                adjusted_music_pos: music,
                current_bpm: 185.0,
                resolution: MUSIC_BAR_DIVISION_COUNT,
            };
            let is_timeout = clock.late_seconds(tick).unwrap() > MISS_SECOND_INTERVAL;
            let since = music - tick as f32;
            let until = next as f32 - music;
            let is_head = is_timeout || until <= 0.0 || since > until;
            let is_midpoint = until > 0.0 && since > until;
            assert_eq!(is_timeout, frame >= timeout);
            assert_eq!(is_head, frame >= head);
            assert_eq!(is_midpoint, midpoint.0 <= frame && frame < midpoint.1);
        }
    }

    #[test]
    fn expiry_enclosure_contains_native_reducer_for_stop_ranges() {
        let trace = FixedBpmMusicTrace::new(185.0, 600).unwrap();
        let ticks = [384, 432, 480];
        let chain = NativeGreenChain {
            root_batch: 0,
            nodes: ticks
                .into_iter()
                .enumerate()
                .map(|(i, tick)| NativeGreenNode {
                    absolute_position: tick,
                    note_bpm: 185.0,
                    note_index: Some(i),
                    role: match i {
                        0 => GreenRole::Head,
                        1 => GreenRole::HeldMiddle,
                        _ => GreenRole::PlainEnd,
                    },
                })
                .collect(),
        };
        let ranges = [
            FrameRange::new(300, 303).unwrap(),
            FrameRange::new(350, 353).unwrap(),
            FrameRange::new(400, 403).unwrap(),
        ];
        let expiry = constant_bpm_expiry_bounds(&chain, &ranges, &trace).unwrap();
        for a in 300..=303 {
            for b in 350..=353 {
                for c in 400..=403 {
                    let stops = [a, b, c];
                    for (i, &tick) in ticks.iter().enumerate() {
                        let mut life = UnsettledLifetime::default();
                        let actual = (stops[i]..=trace.last_update_frame())
                            .find(|&frame| {
                                let clock = NativeClock {
                                    adjusted_music_pos: trace.position_after_update(frame).unwrap(),
                                    current_bpm: 185.0,
                                    resolution: MUSIC_BAR_DIVISION_COUNT,
                                };
                                let update = match i {
                                    0 => LifetimeUpdate::HeadWait {
                                        clock,
                                        next_visible_pos: ticks[1],
                                    },
                                    1 => LifetimeUpdate::AfterStop {
                                        clock,
                                        has_direct_after: true,
                                        next_visible: Some(VisibleAfter {
                                            absolute_pos: ticks[2],
                                            state: if stops[2] <= frame {
                                                VisibleState::Stop
                                            } else {
                                                VisibleState::Move
                                            },
                                        }),
                                        arrow_terminal: false,
                                        execute_frame: 0.5,
                                        adjust_b: 0,
                                    },
                                    _ => LifetimeUpdate::AfterStop {
                                        clock,
                                        has_direct_after: false,
                                        next_visible: None,
                                        arrow_terminal: false,
                                        execute_frame: 0.5,
                                        adjust_b: 0,
                                    },
                                };
                                life.update(tick, update).unwrap().is_some()
                            })
                            .unwrap();
                        assert!(expiry[i].lower <= actual && actual <= expiry[i].upper);
                    }
                }
            }
        }
        let midpoint_end = trace
            .after_midpoint_window(ticks[1], ticks[2])
            .unwrap()
            .unwrap()
            .1;
        let crossing = [
            ranges[0],
            FrameRange::new(midpoint_end - 2, midpoint_end + 2).unwrap(),
            ranges[2],
        ];
        let crossing_expiry = constant_bpm_expiry_bounds(&chain, &crossing, &trace).unwrap();
        for stop in crossing[1].lower..=crossing[1].upper {
            let mut life = UnsettledLifetime::default();
            let actual = (stop..=trace.last_update_frame())
                .find(|&frame| {
                    let update = LifetimeUpdate::AfterStop {
                        clock: NativeClock {
                            adjusted_music_pos: trace.position_after_update(frame).unwrap(),
                            current_bpm: 185.0,
                            resolution: MUSIC_BAR_DIVISION_COUNT,
                        },
                        has_direct_after: true,
                        next_visible: Some(VisibleAfter {
                            absolute_pos: ticks[2],
                            state: if ranges[2].lower <= frame {
                                VisibleState::Stop
                            } else {
                                VisibleState::Move
                            },
                        }),
                        arrow_terminal: false,
                        execute_frame: 0.5,
                        adjust_b: 0,
                    };
                    life.update(ticks[1], update).unwrap().is_some()
                })
                .unwrap();
            assert!(crossing_expiry[1].lower <= actual && actual <= crossing_expiry[1].upper);
        }
    }
}
