//! Certify the same remaining-assignment cutoff without recomputing its DP.
//! The suffix table contains only this roster's power-independent coefficients.
//! Near a floating equality boundary the original forward DP remains decisive.
use super::*;

pub(super) struct Caps {
    caps: [[f64; 6]; 5],
    suffix: [Option<f64>; 32],
    enabled: bool,
}
impl Caps {
    pub(super) fn new(caps: [[f64; 6]; 5]) -> Self {
        Self { enabled: caps.iter().flatten().all(|v| v.is_finite() && *v >= 0.)
                && std::env::var("BANDORI_TIMING_DISABLE_REMAINING_BRACKETS")
                    .as_deref() != Ok("1"),
            caps, suffix: [None; 32] }
    }
    fn exact(&self, prefix: f64, used: u8, stats: &mut TimingAdjacentStats) -> f64 {
        stats.remaining_cap_exact_queries += 1;
        let count = 5-used.count_ones();
        stats.remaining_cap_transitions += u64::from(if count == 0 { 0 }
            else { count * (1 << (count-1)) });
        joint::remaining_cap(prefix, used, &self.caps)
    }
    pub(super) fn prunes(&mut self, prefix: f64, used: u8, incumbent: f64,
        stats: &mut TimingAdjacentStats) -> bool {
        stats.remaining_cap_queries += 1;
        if !self.enabled { return self.exact(prefix, used, stats) <= incumbent; }
        let suffix = if let Some(value) = self.suffix[usize::from(used)] { value }
            else {
                let value = self.exact(0., used, stats);
                self.suffix[usize::from(used)] = Some(value);
                stats.remaining_cap_suffix_builds += 1;
                value
            };
        if prefix == 0. {
            stats.remaining_cap_fast_queries += 1;
            return suffix <= incumbent;
        }
        if let Some((low, high)) = bracket(prefix, suffix) {
            if high <= incumbent { stats.remaining_cap_fast_queries += 1; return true; }
            if low > incumbent { stats.remaining_cap_fast_queries += 1; return false; }
        }
        self.exact(prefix, used, stats) <= incumbent
    }
}

fn bracket(prefix: f64, suffix: f64) -> Option<(f64, f64)> {
    let sum = prefix + suffix;
    if !(prefix >= 0. && suffix >= 0. && sum.is_finite()) { return None; }
    // At most six nonnegative additions. Each round-to-nearest plus next_up
    // is within (1+2*EPSILON) of the real sum, plus one subnormal unit. Both
    // the zero-prefix suffix and the actual prefix sequence are enclosed by
    // this wider 32*EPSILON factor and 32 subnormal units. Every operation
    // below is itself rounded outwards. The score calculation is untouched.
    let inflation = 1. + 32. * f64::EPSILON;
    let tiny = f64::from_bits(32);
    Some((((sum.next_down()/inflation).next_down()-tiny).next_down().max(0.),
        ((sum.next_up()*inflation).next_up()+tiny).next_up()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn brackets_keep_exact_cutoffs_at_ties_and_adjacent_floats_across_all_masks() {
        let mut seed = 128u64;
        let mut random = || { seed ^= seed << 13; seed ^= seed >> 7;
            seed ^= seed << 17; seed };
        for case in 0..600 {
            let rows: [[f64;6];5] = std::array::from_fn(|_| std::array::from_fn(|_| {
                let r = random();
                match case % 12 {
                    0 => 0.,
                    1 => f64::from_bits(r%400),
                    2 => f64::MIN_POSITIVE*(1.+(r%80) as f64/127.),
                    3 => (r%5) as f64 * 0.07,
                    4 => 1e200*(r%400) as f64,
                    5 => 1e-200*(r%400) as f64,
                    _ => (r%400) as f64 * 0.031,
                }
            }));
            let mut caps = Caps { enabled: true, ..Caps::new(rows) };
            let mut stats = TimingAdjacentStats::default();
            for used in 0..32 {
                let suffix = joint::remaining_cap(0.,used,&rows);
                for prefix in [0.,0.001,0.17,1.,50.,f64::MIN_POSITIVE,
                    f64::from_bits(1),1e100,1e-300,1e250] {
                    let expected = joint::remaining_cap(prefix,used,&rows);
                    let (low,high) = bracket(prefix,suffix).unwrap();
                    assert!(low <= expected && expected <= high);
                    for incumbent in [expected,expected.next_up(),expected.next_down(),
                        expected*0.9,expected*1.1] {
                        assert_eq!(caps.prunes(prefix,used,incumbent,&mut stats), expected<=incumbent);
                    }
                }
            }
        }
    }
    #[test]
    fn negative_caps_and_nonfinite_brackets_preserve_the_original_forward_dp() {
        for rows in [[[-0.01;6];5], [[f64::INFINITY;6];5]] {
            let mut caps = Caps::new(rows);
            assert!(!caps.enabled);
            let mut stats = TimingAdjacentStats::default();
            for used in 0..32 { for prefix in [0., 0.1, -0.5] {
                let expected = joint::remaining_cap(prefix, used, &rows);
                for incumbent in [0., 0.1, 1., expected] {
                    assert_eq!(caps.prunes(prefix,used,incumbent,&mut stats),expected<=incumbent);
                }
            } }
        }
        assert!(bracket(-1.,1.).is_none());
        assert!(bracket(f64::MAX,f64::MAX).is_none());
    }
}
