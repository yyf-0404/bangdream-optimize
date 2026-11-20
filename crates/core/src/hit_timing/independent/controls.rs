//! Immutable experimental switches for one prepared scoring model. This stores
//! only configuration, never native states, scores or answers between queries.

macro_rules! switches {
    ($($variant:ident => ($name:literal, $opt_in:literal)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(super) enum Switch { $($variant),+ }
        impl Switch {
            const ALL: &'static [Self] = &[$(Self::$variant),+];
            fn specification(self) -> (&'static str, bool) {
                match self { $(Self::$variant => (concat!("BANDORI_TIMING_DISABLE_", $name), $opt_in)),+ }
            }
            fn read(self) -> bool {
                let (name, opt_in) = self.specification();
                let value = std::env::var(name);
                Self::interpret(value.as_deref().ok(), opt_in)
            }
        }
    };
}

switches! {
    ValueArena => ("VALUE_ARENA", false),
    BlockReductions => ("BLOCK_REDUCTIONS", false),
    UniformBlocks => ("UNIFORM_BLOCKS", false),
    StaticBlockOperators => ("STATIC_BLOCK_OPERATORS", false),
    InlineReductions => ("INLINE_REDUCTIONS", false),
    NativeClockCells => ("NATIVE_CLOCK_CELLS", false),
    PartialTemporal => ("PARTIAL_TEMPORAL", false),
    IdleTemporal => ("IDLE_TEMPORAL", false),
    FrontierLengths => ("FRONTIER_LENGTHS", false),
    ValueBlocks => ("VALUE_BLOCKS", false),
    FastValueStats => ("FAST_VALUE_STATS", false),
    ClockBuckets => ("CLOCK_BUCKETS", false),
    ContinuousDominance => ("CONTINUOUS_DOMINANCE", false),
    ContinuousWindowDominance => ("CONTINUOUS_WINDOW_DOMINANCE", false),
    WindowDominance => ("WINDOW_DOMINANCE", false),
    ProgressDominance => ("PROGRESS_DOMINANCE", false),
    JointPrecompute => ("JOINT_PRECOMPUTE", false),
    OutsideScoreProgram => ("OUTSIDE_SCORE_PROGRAM", false),
    LazyJointScores => ("LAZY_JOINT_SCORES", false),
    InternedClocks => ("INTERNED_CLOCKS", false),
    FrameOperators => ("FRAME_OPERATORS", false),
    TouchDag => ("TOUCH_DAG", false),
    WindowPareto => ("WINDOW_PARETO", true),
    ClockStates => ("CLOCK_STATES", true),
    ClockAliases => ("CLOCK_ALIASES", true),
    QueuedUniform => ("QUEUED_UNIFORM", true),
    ScoreCircuits => ("SCORE_CIRCUITS", true),
    ComponentFunctions => ("COMPONENT_FUNCTIONS", true),
    IncrementalTouch => ("INCREMENTAL_TOUCH", true),
    FusedExits => ("FUSED_EXITS", true),
    NativeKernelFastpaths => ("NATIVE_KERNEL_FASTPATHS", false),
}

impl Switch {
    fn interpret(value: Option<&str>, opt_in: bool) -> bool {
        if opt_in { value == Some("0") } else { value != Some("1") }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Controls { bits: u64 }
impl Controls {
    const DYNAMIC: u64 = 1 << 63;
    pub fn dynamic() -> Self { Self { bits: Self::DYNAMIC } }
    pub fn prepared() -> Self {
        if std::env::var("BANDORI_TIMING_DISABLE_PREPARED_RUNTIME_SWITCHES").as_deref() == Ok("1") {
            Self::dynamic()
        } else { Self::capture(Switch::read) }
    }
    pub fn capture(mut read: impl FnMut(Switch) -> bool) -> Self {
        assert!(Switch::ALL.len() < 63);
        let mut bits = 0;
        for &switch in Switch::ALL {
            if read(switch) { bits |= 1 << switch as u32; }
        }
        Self { bits }
    }
    #[inline]
    pub fn enabled(self, switch: Switch) -> bool {
        if self.bits == Self::DYNAMIC { switch.read() }
        else { self.bits & (1 << switch as u32) != 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_switches_keep_every_bit_and_read_configuration_once() {
        assert_eq!(std::mem::size_of::<Controls>(), 8);
        for chosen in Switch::ALL {
            let mut visits = vec![];
            let saved = Controls::capture(|switch| { visits.push(switch); switch == *chosen });
            assert_eq!(visits, Switch::ALL);
            for &switch in Switch::ALL { assert_eq!(saved.enabled(switch), switch == *chosen); }
        }
        for enabled in [false, true] {
            let saved = Controls::capture(|_| enabled);
            for &switch in Switch::ALL { assert_eq!(saved.enabled(switch), enabled); }
        }
    }
    #[test]
    fn switch_interpretation_preserves_default_on_and_explicit_opt_in() {
        for value in [None, Some(""), Some("0"), Some("1"), Some("invalid"), Some("中文")] {
            assert_eq!(Switch::interpret(value, false), value != Some("1"));
            assert_eq!(Switch::interpret(value, true), value == Some("0"));
        }
    }
}
