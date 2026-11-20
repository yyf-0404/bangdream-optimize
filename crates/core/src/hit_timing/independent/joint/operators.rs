//! Compile identical ordinary value operators once inside a fresh score query.
//! Geometry legality is still evaluated on every edge. Exact frame, raw grade,
//! score band and input state identify the complete native value transition.
//! Direct-mapped storage is bounded; collisions simply recompute the operator.
use super::*;

const SLOTS: usize = 2048;

#[derive(Clone, Copy)]
struct Cell {
    frame: u64,
    state: usize,
    band: usize,
    raw: u8,
    result: Option<(usize, u32)>,
}

pub(super) struct Operators {
    cells: Vec<Cell>,
    hits: u64,
    misses: u64,
    frame_cells: bool,
}

impl Operators {
    pub(super) fn new(controls: Controls) -> Self {
        Self::with_controlled(controls.enabled(Switch::FrameOperators), controls.enabled(Switch::NativeClockCells))
    }

    pub(super) fn with_enabled(enabled: bool) -> Self {
        Self::with_controlled(enabled,std::env::var("BANDORI_TIMING_DISABLE_NATIVE_CLOCK_CELLS").as_deref()!=Ok("1"))
    }
    pub(super) fn with_controlled(enabled:bool,frame_cells:bool) -> Self {
        Self { cells: if enabled { vec![Cell { frame: u64::MAX, state: 0, band: 0,
            raw: 0, result: None }; SLOTS] } else { vec![] }, hits: 0, misses: 0,frame_cells }
    }

    pub(super) fn enabled(&self) -> bool { !self.cells.is_empty() }
    pub(super) fn frame_cells(&self) -> bool { self.frame_cells }
    pub(super) fn frame_key(frame:u32,interval:Option<(u32,u32)>) -> u64 {
        match interval {
            Some((first,last)) if first<=frame&&frame<=last=>(1u64<<32)|u64::from(first),
            _=>u64::from(frame),
        }
    }

    fn index(state: usize, prepared: PreparedAction) -> usize {
        (state.wrapping_mul(0x9e37_79b1) ^ prepared.band.wrapping_mul(0x85eb_ca6b)
            ^ (prepared.raw as usize).wrapping_mul(0xc2b2_ae35)) & (SLOTS - 1)
    }

    pub(super) fn get(&mut self, frame: u64, state: usize, prepared: PreparedAction)
        -> Option<Option<(usize, Value)>> {
        let cell = self.cells[Self::index(state, prepared)];
        if cell.frame == frame && cell.state == state && cell.band == prepared.band
            && cell.raw == prepared.raw as u8 {
            self.hits += 1;
            Some(cell.result.map(|(state, score)| (state,
                Value { score: u64::from(score), coefficient: 0. })))
        } else { self.misses += 1; None }
    }

    pub(super) fn put(&mut self, frame: u64, state: usize, prepared: PreparedAction,
        result: Option<(usize, Value)>) {
        self.cells[Self::index(state, prepared)] = Cell { frame, state, band: prepared.band,
            raw: prepared.raw as u8, result: result.map(|(state, value)| {
                // Every ordinary native note increment has the original u32 domain.
                (state, u32::try_from(value.score).expect("native note score is u32"))
            }) };
    }

    pub(super) fn stats(&self, stats: &mut WindowStats) {
        stats.joint_frame_operator_hits += self.hits;
        stats.joint_frame_operator_misses += self.misses;
        stats.joint_frame_operator_bytes = stats.joint_frame_operator_bytes.max(
            self.cells.capacity() * std::mem::size_of::<Cell>());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_operators_keep_frames_bands_grades_states_rejections_and_queries_distinct() {
        let mut operators = Operators::with_enabled(true);
        let perfect = PreparedAction { raw: Judgement::Perfect, band: 0 };
        let great = PreparedAction { raw: Judgement::Great, band: 0 };
        operators.put(100, 1, perfect, Some((2, Value { score: 123, coefficient: 0. })));
        let result = operators.get(100, 1, perfect).unwrap().unwrap();
        assert_eq!((result.0, result.1.score), (2, 123));
        for (frame, state, prepared) in [(101, 1, perfect), (100, 2, perfect),
            (100, 1, great), (100, 1, PreparedAction { band: 1, ..perfect })] {
            assert!(operators.get(frame, state, prepared).is_none());
        }
        // Multiplication modulo the power-of-two capacity deliberately collides.
        operators.put(100, 1 + SLOTS, perfect, None);
        assert!(operators.get(100, 1, perfect).is_none());
        assert!(matches!(operators.get(100, 1 + SLOTS, perfect), Some(None)));
        assert!(Operators::with_enabled(true).get(100, 1 + SLOTS, perfect).is_none());
        assert!(operators.cells.capacity() * std::mem::size_of::<Cell>() <= 128 * 1024);
    }

    #[test]
    fn clock_cell_keys_keep_the_cell_start_state_and_real_frames_distinct() {
        let a=Operators::frame_key(10,Some((10,20)));
        assert_eq!(a,Operators::frame_key(20,Some((10,20))));
        assert_ne!(a,Operators::frame_key(9,Some((0,9))));
        assert_ne!(a,Operators::frame_key(10,None));
        assert_ne!(Operators::frame_key(u32::MAX,Some((u32::MAX,u32::MAX))),u64::from(u32::MAX));
        let mut ops=Operators::with_controlled(true,true);
        assert!(ops.get(u64::from(u32::MAX),0,PreparedAction {raw:Judgement::Miss,band:0}).is_none());
    }
}
