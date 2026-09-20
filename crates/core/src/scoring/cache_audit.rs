//! Opt-in measurement of cache reuse. Default builds erase these calls.
#[derive(Clone, Copy)]
pub enum Cache {
    SingleScore,
    SinglePower,
    QueuePower,
    QueueDelta,
    ControlScalar,
    ControlBatch,
    CooperativeScore,
    MedleyDistribution,
    MedleySeed,
    PowerPlateau,
    ControlBatchProbe,
}

#[cfg(feature = "experimental-cache-audit")]
const NAMES: [&str; 11] = [
    "single-score",
    "single-power",
    "queue-power",
    "queue-delta",
    "control-scalar",
    "control-batch",
    "cooperative-score",
    "medley-distribution",
    "medley-seed",
    "power-plateau",
    "control-batch-probe",
];

#[inline]
pub fn enabled(cache: Cache) -> bool {
    #[cfg(feature = "experimental-cache-audit")]
    {
        static DISABLED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
        let disabled = DISABLED.get_or_init(|| {
            let value = std::env::var("BANGDREAM_OPTIMIZE_DISABLE_CACHES").unwrap_or_default();
            NAMES.iter().enumerate().fold(0, |mask, (i, name)| {
                mask | if value.split(',').any(|part| part == *name) {
                    1 << i
                } else {
                    0
                }
            })
        });
        disabled & (1 << cache as u32) == 0
    }
    #[cfg(not(feature = "experimental-cache-audit"))]
    {
        let _ = cache;
        true
    }
}

#[cfg(feature = "experimental-cache-audit")]
#[derive(Clone, Copy, Default, serde::Serialize)]
struct Counts {
    hits: u64,
    misses: u64,
    peak_entries: usize,
}

#[cfg(feature = "experimental-cache-audit")]
thread_local! {
    static COUNTS: std::cell::RefCell<[Counts; 11]> = std::cell::RefCell::new([Counts::default(); 11]);
}

#[inline]
pub fn lookup(cache: Cache, hit: bool, entries: usize) {
    #[cfg(feature = "experimental-cache-audit")]
    COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        let c = &mut counts[cache as usize];
        if hit {
            c.hits += 1;
        } else {
            c.misses += 1;
        }
        c.peak_entries = c.peak_entries.max(entries);
    });
    #[cfg(not(feature = "experimental-cache-audit"))]
    {
        let _ = (cache, hit, entries);
    }
}

pub fn report() {
    #[cfg(feature = "experimental-cache-audit")]
    COUNTS.with(|counts| {
        for (name, counts) in NAMES.iter().zip(counts.borrow().iter()) {
            eprintln!(
                "CACHE_AUDIT {name} {}",
                serde_json::to_string(counts).unwrap()
            );
        }
    });
}
