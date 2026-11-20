//! Finite-frame envelopes for ONE independent request. Every note in a row
//! shares that request time. Note routing, rank and history are relaxed only
//! for bounds: these rows are not playable optima or heuristic scores.
use super::*;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub(crate) time: u32,
    pub(crate) raw: Judgement,
    pub(crate) weight: [f64; 2], // Level / count * 3 * Combo * Fever, before judgement.
}

#[derive(Clone)]
struct Group {
    outside: bool,
    // Common Great/Perfect, then additional-tail Great/Perfect.
    weights: [[f64; 2]; 4],
    count: usize,
}

type Support = [Vec<Vec<Group>>; 6];

fn base_scale(level: i32, count: u32) -> [f64; 2] {
    let rate = f64::from(1.0_f32 + (level - 5) as f32 * 0.01_f32);
    // The native divisor is the rounded f32 count, including above 2^24.
    let divisor = f64::from(count as f32);
    [
        ((rate / divisor).next_down() * 3.).next_down(),
        ((rate / divisor).next_up() * 3.).next_up(),
    ]
}

#[derive(Clone)]
pub(crate) struct FrameBounds {
    pub(crate) notes: Arc<Vec<Vec<Frame>>>,
    references: Arc<Vec<f64>>,
    cached_references: bool,
    spans: Arc<Vec<(u32, u32)>>,
    pub(in crate::hit_timing::bounds) interval_index: Arc<super::super::net::intervals::Index>,
    indexed: bool,
    pub(crate) triggers: Arc<Vec<Option<usize>>>,
    pub(crate) requests: [Vec<u32>; 6],
    support: BTreeMap<(u32, u32, u32), Arc<Support>>,
    differences: HashMap<(usize, usize), [f64; 6]>,
    pub(super) shared: Option<SharedFrameCoefficients>,
    pub base: [f64; 2],
}

pub(crate) fn corrected(raw: Judgement, correction: u32) -> Option<usize> {
    if correction > 0 {
        let threshold = match correction {
            1 => Judgement::Great,
            2 => Judgement::Good,
            _ => Judgement::Bad,
        };
        return (raw >= threshold).then_some(1);
    }
    match raw {
        Judgement::Great => Some(0),
        Judgement::Perfect => Some(1),
        _ => None,
    }
}

impl FrameBounds {
    pub(crate) fn delay_requests(&mut self, skills: &[ReplaySkill]) {
        let shortest = skills
            .iter()
            .map(|s| countdown(s.duration) + 1)
            .min()
            .unwrap();
        let longest = skills
            .iter()
            .map(|s| countdown(s.duration) + 1)
            .max()
            .unwrap();
        let cooling = countdown(0.75) + 1;
        let mut previous: Option<(u32, u32)> = None;
        for requests in &mut self.requests {
            // Search domains are ordered by distance from the nominal hit,
            // not by time. Never use their first/last entries as endpoints.
            let (mut first, mut last) = (
                *requests.iter().min().unwrap(),
                *requests.iter().max().unwrap(),
            );
            if let Some((a, b)) = previous {
                first = first.max(a + 1);
                last = last.max(b + 1);
            }
            previous = Some((first + shortest + cooling, last + longest + cooling));
            *requests = (first..=last).collect();
        }
    }
    /// A common reference for every window, computed once per note. A note
    /// outside ALL skills has raw P/G and cannot exceed this coefficient.
    pub(crate) fn note_baselines(&self) -> Vec<f64> {
        if self.cached_references {self.references.as_ref().clone()}
        else {Self::reference_coefficients(&self.notes)}
    }
    fn reference_coefficients(notes: &[Vec<Frame>]) -> Vec<f64> {
        notes
            .iter()
            .map(|n| {
                n.iter()
                    .filter(|f| f.raw >= Judgement::Great)
                    .map(|f| (f.weight[1] * f64::from(f.raw.multiplier())).next_up())
                    .fold(0., f64::max)
            })
            .collect()
    }
    pub fn new(input: &ReplayInput, skills: &[ReplaySkill]) -> Result<Self, ReplayError> {
        Self::new_controlled(input, skills,
            std::env::var("BANDORI_TIMING_DISABLE_FRAME_COMBO_EXTREMA").as_deref() != Ok("1"))
    }
    fn new_controlled(input: &ReplayInput, skills: &[ReplaySkill], endpoints: bool)
        -> Result<Self, ReplayError> {
        let mut widened = input.clone();
        let strongest = skills.iter().max_by_key(|s| s.cache_key()[1]).unwrap();
        widened.skills = vec![strongest.clone(); 6];
        let domains = search::frame_domains(&widened)?;
        let mut first: Vec<_> = domains.iter().map(|d| *d.iter().min().unwrap()).collect();
        let mut last: Vec<_> = domains.iter().map(|d| *d.iter().max().unwrap()).collect();
        first.sort_unstable();
        last.sort_unstable();
        let combos: Vec<_> = (1..=domains.len())
            .map(|n| f64::from(input.combo_multiplier(input.initial_combo + n as u32)))
            .collect();
        let monotone = endpoints && combos.iter().all(|&c| c.is_finite() && c > 0.)
            && combos.windows(2).all(|w| w[0] <= w[1]);
        let scale = base_scale(input.play_level, domains.len() as u32);
        let mut base = [0_f64; 2];
        let mut notes = Vec::new();
        let mut requests: [Vec<u32>; 6] = std::array::from_fn(|_| vec![]);
        for (i, domain) in domains.iter().enumerate() {
            if let Some(s) = input.notes[i].trigger {
                requests[s] = domain.clone();
            }
            let mut frames = vec![];
            let mut baseline = [f64::INFINITY, 0_f64];
            for &time in domain {
                // At least all notes whose LAST possible frame is strictly
                // earlier, plus this note. Ties stay relaxed in both directions.
                let low_rank = last.partition_point(|&f| f < time);
                let high_rank = first.partition_point(|&f| f <= time);
                let range = &combos[low_rank..high_rank];
                let combo = if monotone && !range.is_empty() {
                    [range[0],range[range.len()-1]]
                } else { [
                    range.iter().copied().fold(f64::INFINITY, f64::min),
                    range.iter().copied().fold(0., f64::max),
                ] };
                let fever = if input
                    .fever_frames
                    .is_some_and(|[a, b]| a <= time && time < b)
                {
                    2.
                } else {
                    1.
                };
                let weight = [
                    ((scale[0] * combo[0]).next_down() * fever).next_down(),
                    ((scale[1] * combo[1]).next_up() * fever).next_up(),
                ];
                let raw = input.notes[i].raw_judgement(time)?;
                // An accepted raw Good/Bad must have been corrected to P.
                let minimum_grade = if raw >= Judgement::Great {
                    raw
                } else {
                    Judgement::Perfect
                };
                baseline[0] = baseline[0]
                    .min((weight[0] * f64::from(minimum_grade.multiplier())).next_down());
                baseline[1] = baseline[1]
                    .max((weight[1] * f64::from(Judgement::Perfect.multiplier())).next_up());
                frames.push(Frame { time, raw, weight });
            }
            base[0] = (base[0] + baseline[0]).next_down();
            base[1] = (base[1] + baseline[1]).next_up();
            notes.push(frames);
        }
        let spans = notes
            .iter()
            .map(|n| {
                (
                    n.iter().map(|f| f.time).min().unwrap(),
                    n.iter().map(|f| f.time).max().unwrap(),
                )
            })
            .collect();
        let interval_index = Arc::new(super::super::net::intervals::Index::new(&notes));
        let references=Arc::new(Self::reference_coefficients(&notes));
        Ok(Self {
            notes: Arc::new(notes),
            references,
            cached_references:std::env::var("BANDORI_TIMING_DISABLE_NOTE_REFERENCE_CACHE").as_deref()!=Ok("1"),
            spans: Arc::new(spans),
            interval_index,
            indexed: std::env::var("BANDORI_TIMING_DISABLE_REPLACEMENT_INTERVAL_INDEX")
                .as_deref() != Ok("1"),
            triggers: Arc::new(input.notes.iter().map(|n| n.trigger).collect()),
            requests,
            support: BTreeMap::new(),
            differences: HashMap::new(),
            shared: None,
            base,
        })
    }

    pub(crate) fn geometry_bytes(&self) -> usize {
        // Only pristine immutable geometry is retained by the NET owner.
        debug_assert!(self.support.is_empty() && self.differences.is_empty());
        std::mem::size_of::<Self>() + 10*std::mem::size_of::<usize>() + 4*std::mem::size_of::<Vec<u8>>()
            + self.references.capacity()*std::mem::size_of::<f64>()
            + self.notes.capacity()*std::mem::size_of::<Vec<Frame>>()
            + self.notes.iter().map(|n| n.capacity()*std::mem::size_of::<Frame>()).sum::<usize>()
            + self.spans.capacity()*std::mem::size_of::<(u32,u32)>()
            + self.triggers.capacity()*std::mem::size_of::<Option<usize>>()
            + self.requests.iter().map(|r| r.capacity()*std::mem::size_of::<u32>()).sum::<usize>()
            + self.interval_index.storage_bytes()
    }

    fn support(
        &mut self,
        short: u32,
        long: u32,
        correction: u32,
        stats: &mut TimingReplacementStats,
    ) -> Arc<Support> {
        let key = (short, long, correction);
        if let Some(v) = self.support.get(&key) {
            stats.frame_support_hits += 1;
            return v.clone();
        }
        let mut support: Support = std::array::from_fn(|_| vec![]);
        for (slot, rows) in support.iter_mut().enumerate() {
            for &request in &self.requests[slot] {
                let mut grouped = BTreeMap::<[u64; 9], (Group, usize)>::new();
                let candidates = if self.indexed {
                    self.interval_index.query(request + 1, request + long)
                } else { (0..self.notes.len()).collect() };
                for i in candidates {
                    let frames = &self.notes[i];
                    if self.triggers[i].is_some_and(|s| s <= slot) {
                        continue;
                    }
                    if self.spans[i].1 <= request || self.spans[i].0 > request + long {
                        continue;
                    }
                    let mut group = Group {
                        outside: false,
                        weights: [[f64::INFINITY, f64::NEG_INFINITY]; 4],
                        count: 1,
                    };
                    for f in frames {
                        if f.time <= request || f.time > request + long {
                            group.outside = true;
                            continue;
                        }
                        let Some(grade) = corrected(f.raw, correction) else {
                            continue;
                        };
                        let region = usize::from(f.time > request + short);
                        let factor = f64::from(
                            if grade == 0 {
                                Judgement::Great
                            } else {
                                Judgement::Perfect
                            }
                            .multiplier(),
                        );
                        let w = &mut group.weights[2 * region + grade];
                        w[0] = w[0].min((f.weight[0] * factor).next_down());
                        w[1] = w[1].max((f.weight[1] * factor).next_up());
                    }
                    if !group.outside && !group.weights.iter().any(|w| w[0].is_finite()) {
                        // No accepted input in this relaxed row. This request
                        // is impossible, but retaining zero contribution is a
                        // deliberately wider envelope, not an eligibility cut.
                        group.outside = true;
                    }
                    let bits = std::array::from_fn(|k| {
                        if k == 8 {
                            u64::from(group.outside)
                        } else {
                            group.weights[k / 2][k % 2].to_bits()
                        }
                    });
                    grouped
                        .entry(bits)
                        .and_modify(|(_, n)| *n += 1)
                        .or_insert((group, 1));
                }
                let groups: Vec<_> = grouped
                    .into_values()
                    .map(|(mut g, n)| {
                        g.count = n;
                        g
                    })
                    .collect();
                stats.frame_note_groups += groups.len();
                stats.frame_start_rows += 1;
                rows.push(groups);
            }
        }
        stats.frame_support_profiles += 1;
        let support = Arc::new(support);
        self.support.insert(key, support.clone());
        support
    }

    pub(super) fn profile(
        &mut self,
        id: usize,
        p: &Profile,
        stats: &mut TimingReplacementStats,
    ) -> [[f64; 2]; 10] {
        if let Some(rows) = self.shared.as_ref().and_then(|c| c.lock().unwrap().profile(id)) {
            stats.frame_coefficient_hits += 1;
            return rows;
        }
        let support = self.support(p.playing, p.playing, p.correction, stats);
        let mut coefficients = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
        for row in p.rows() {
            for k in 0..4 {
                if p.correction != 0 && k != 1 {
                    continue;
                }
                let v = f64::from(row[k]) - 1.;
                coefficients[k % 2][0] = coefficients[k % 2][0].min(v.next_down());
                coefficients[k % 2][1] = coefficients[k % 2][1].max(v.next_up());
            }
        }
        let values: [[f64; 2]; 6] = std::array::from_fn(|slot| {
            envelope(
                &support[slot],
                [coefficients[0], coefficients[1], [0.; 2], [0.; 2]],
            )
        });
        let rows = std::array::from_fn(|s| {
            if s < 5 {
                values[s]
            } else {
                [
                    (values[s - 5][0] + values[5][0]).next_down(),
                    (values[s - 5][1] + values[5][1]).next_up(),
                ]
            }
        });
        if let Some(cache) = &self.shared { cache.lock().unwrap().store_profile(id,rows); }
        rows
    }

    pub(super) fn difference(
        &mut self,
        new: usize,
        old: usize,
        profiles: &[Profile],
        stats: &mut TimingReplacementStats,
    ) -> [f64; 6] {
        if let Some(&value) = self.differences.get(&(new, old)) {
            return value;
        }
        let key = [0,new,old];
        if let Some(rows) = self.shared.as_ref().and_then(|c| c.lock().unwrap().pair(key)) {
            self.differences.insert((new,old),rows);
            stats.frame_coefficient_hits += 1;
            return rows;
        }
        let start = Instant::now();
        let a = &profiles[new];
        let b = &profiles[old];
        // The caller has already checked same correction, and excludes
        // correction-changing tails. This contains no adjacent-skill test.
        let support = self.support(
            a.playing.min(b.playing),
            a.playing.max(b.playing),
            a.correction,
            stats,
        );
        let mut c = [f64::INFINITY; 4];
        for (x, y) in a.paired_rows(b) {
            for k in 0..4 {
                if a.correction != 0 && k != 1 {
                    continue;
                }
                c[k % 2] = c[k % 2].min((f64::from(x[k]) - f64::from(y[k])).next_down());
                let tail = if a.playing > b.playing {
                    f64::from(x[k]) - 1.
                } else {
                    1. - f64::from(y[k])
                };
                c[2 + k % 2] = c[2 + k % 2].min(tail.next_down());
            }
        }
        let result = std::array::from_fn(|slot| lower_envelope(&support[slot], c.map(|v| [v, v])));
        self.differences.insert((new, old), result);
        if let Some(cache) = &self.shared { cache.lock().unwrap().store_pair(key,result); }
        stats.frame_pair_queries += 1;
        stats.frame_query_seconds += start.elapsed().as_secs_f64();
        result
    }

    /// Bounds the SAME raw input, including the change in judgement weight.
    /// Histories are independent here: a corrected Great increments Perfect
    /// counts and can keep a continuous skill clean. Pairing equal counts or
    /// equal clean/spoiled flags across the two skills would be unsound.
    pub(super) fn correction_difference(
        &mut self,
        a: &Profile,
        b: &Profile,
        stats: &mut TimingReplacementStats,
    ) -> [f64; 6] {
        assert_eq!(a.playing, b.playing);
        assert!(b.correction <= 1 || a.correction >= b.correction);
        let start = Instant::now();
        let extrema = |p: &Profile, grade: usize, minimum: bool| {
            p.rows()
                .iter()
                .flat_map(|r| [r[grade], r[grade + 2]])
                .map(f64::from)
                .fold(
                    if minimum {
                        f64::INFINITY
                    } else {
                        f64::NEG_INFINITY
                    },
                    |a, b| if minimum { a.min(b) } else { a.max(b) },
                )
        };
        let effective_low = |grade: usize| {
            let multiplier = if grade == 0 {
                Judgement::Great
            } else {
                Judgement::Perfect
            }
            .multiplier();
            (extrema(a, grade, true) * f64::from(multiplier)).next_down()
        };
        let delta = std::array::from_fn::<_, 2, _>(|old_grade| {
            let new_total = if a.correction > 0 {
                effective_low(1)
            } else if b.correction > 0 {
                // Old P includes corrected raw Great and genuine Perfect.
                effective_low(0).min(effective_low(1))
            } else {
                effective_low(old_grade)
            };
            let old_weight = f64::from(
                if old_grade == 0 {
                    Judgement::Great
                } else {
                    Judgement::Perfect
                }
                .multiplier(),
            );
            ((new_total / old_weight).next_down() - extrema(b, old_grade, false)).next_down()
        });
        let support = self.support(b.playing, b.playing, b.correction, stats);
        let result = std::array::from_fn(|s| {
            lower_envelope(
                &support[s],
                [[delta[0]; 2], [delta[1]; 2], [0.; 2], [0.; 2]],
            )
        });
        stats.frame_pair_queries += 1;
        stats.frame_query_seconds += start.elapsed().as_secs_f64();
        result
    }
}

// Each request uses one shared row. Notes and their histories may choose
// separately within it, which is a bound rather than an executable optimum.
fn envelope(rows: &[Vec<Group>], coefficients: [[f64; 2]; 4]) -> [f64; 2] {
    let mut result = [f64::INFINITY, f64::NEG_INFINITY];
    for groups in rows {
        let mut sum = [0_f64; 2];
        for g in groups {
            let mut value = if g.outside {
                [0., 0.]
            } else {
                [f64::INFINITY, f64::NEG_INFINITY]
            };
            for (w, c) in g.weights.iter().zip(&coefficients) {
                if !w[0].is_finite() {
                    continue;
                }
                value[0] = value[0].min(product_lower(*w, *c));
                value[1] = value[1].max(-product_lower(*w, [-c[1], -c[0]]));
            }
            sum[0] = (sum[0] + (value[0] * g.count as f64).next_down()).next_down();
            sum[1] = (sum[1] + (value[1] * g.count as f64).next_up()).next_up();
        }
        result[0] = result[0].min(sum[0]);
        result[1] = result[1].max(sum[1]);
    }
    result
}

// Replacement certificates consume only the lower endpoint. Preserve the
// exact original lower operations and group order without computing an unused
// upper endpoint at every skill-pair query.
fn lower_envelope(rows: &[Vec<Group>], coefficients: [[f64; 2]; 4]) -> f64 {
    let mut result = f64::INFINITY;
    for groups in rows {
        let mut sum = 0_f64;
        for g in groups {
            let mut value = if g.outside { 0. } else { f64::INFINITY };
            for (w, c) in g.weights.iter().zip(&coefficients) {
                if w[0].is_finite() { value = value.min(product_lower(*w, *c)); }
            }
            sum = (sum + (value * g.count as f64).next_down()).next_down();
        }
        result = result.min(sum);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base_scale_encloses_native_divisor_on_both_sides_of_f32_integer_rounding() {
        use crate::scoring::NoteScorer;
        for count in [1, 760, (1 << 24) - 1, 1 << 24,
            (1 << 24) + 1, (1 << 24) + 3, (1 << 25) + 3, u32::MAX] {
            // At level 5 and power=f32(count), native division is exactly 1.
            // This checks the continuous coefficient against native arithmetic
            // without constructing a chart with millions of individual notes.
            let power = ScoringPower::new(count as f32).unwrap();
            let base = NoteScorer::new(power, 5, count).unwrap().base();
            assert_eq!(base, 3.);
            let native = f64::from(base) / f64::from(power.value());
            let scale = base_scale(5, count);
            assert!(scale[0] <= native && native <= scale[1], "count={count}");
            let old = [
                ((1. / f64::from(count)).next_down() * 3.).next_down(),
                ((1. / f64::from(count)).next_up() * 3.).next_up(),
            ];
            if count <= 1 << 24 {
                assert_eq!(scale.map(f64::to_bits), old.map(f64::to_bits));
            }
            if count == (1 << 24) + 1 {
                assert!(old[1] < native, "rounded-down count needs a larger upper");
            }
            if count == (1 << 24) + 3 {
                assert!(old[0] > native, "rounded-up count needs a smaller lower");
            }
        }
    }
    #[test]
    fn fixed_note_references_keep_bits_across_domains_combo_fever_and_delayed_clones() {
        let catalogue=super::super::tests::catalogue();
        for medley in [false,true] {for combo in [0,49,699,999] {
            for fever in [None,Some([6,24])] {for skill in &catalogue {
                let mut input=super::super::tests::fixture(0.045);
                input.medley_combo=medley;input.initial_combo=combo;input.fever_frames=fever;
                let skills=vec![skill.clone();6];
                let mut fresh=FrameBounds::new(&input,&skills).unwrap();
                let expected:Vec<_>=fresh.notes.iter().map(|n|n.iter()
                    .filter(|f|f.raw>=Judgement::Great)
                    .map(|f|(f.weight[1]*f64::from(f.raw.multiplier())).next_up())
                    .fold(0.,f64::max).to_bits()).collect();
                assert_eq!(fresh.note_baselines().iter().map(|v|v.to_bits()).collect::<Vec<_>>(),expected);
                fresh.cached_references=false;
                assert_eq!(fresh.note_baselines().iter().map(|v|v.to_bits()).collect::<Vec<_>>(),expected);
                let mut delayed=fresh.clone();delayed.cached_references=true;
                delayed.delay_requests(&skills);
                assert_eq!(delayed.note_baselines().iter().map(|v|v.to_bits()).collect::<Vec<_>>(),expected);
                assert!(Arc::ptr_eq(&fresh.references,&delayed.references));
            }}
        }}
    }
    #[test]
    fn monotone_combo_endpoints_match_rank_scans_at_every_frame() {
        let skills = super::super::tests::catalogue();
        for spacing in [0.01, 0.045, 0.19] {
            for medley in [false, true] {
                for initial_combo in [0, 48, 98, 198, 698] {
                    let mut input = super::super::tests::fixture(spacing);
                    input.medley_combo = medley;
                    input.initial_combo = initial_combo;
                    let scan = FrameBounds::new_controlled(&input, &skills, false).unwrap();
                    let endpoints = FrameBounds::new_controlled(&input, &skills, true).unwrap();
                    assert_eq!(scan.base.map(f64::to_bits), endpoints.base.map(f64::to_bits));
                    assert_eq!(scan.requests, endpoints.requests);
                    for (a, b) in scan.notes.iter().zip(endpoints.notes.iter()) {
                        assert_eq!(a.len(), b.len());
                        for (a, b) in a.iter().zip(b) {
                            assert_eq!((a.time, a.raw, a.weight.map(f64::to_bits)),
                                (b.time, b.raw, b.weight.map(f64::to_bits)));
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn lower_only_certificate_matches_both_endpoint_implementation_bit_for_bit() {
        let rows = vec![vec![
            Group { outside: true, weights: [[0.3, 0.4], [0.5, 0.7],
                [f64::INFINITY, f64::NEG_INFINITY], [0.01, 0.04]], count: 3 },
            Group { outside: false, weights: [[0.1, 0.15], [0.2, 0.4],
                [0.9, 1.2], [0.002, 0.003]], count: 2 },
        ], vec![], vec![Group { outside: true,
            weights: [[f64::INFINITY, f64::NEG_INFINITY]; 4], count: 1 }]];
        for a in [-1_f64, -0.002, 0., 0.001, 1.3] {
            for b in [-1_f64, -0.002, 0., 0.001, 1.3] {
                let coefficients = [[a, a], [b, b], [a.min(b), a.max(b)], [0., 0.]];
                assert_eq!(lower_envelope(&rows, coefficients).to_bits(),
                    envelope(&rows, coefficients)[0].to_bits());
            }
        }
    }
    #[test]
    fn indexed_and_exhaustive_support_keep_all_endpoint_bits() {
        let input = super::super::tests::fixture(0.045);
        let skills = super::super::tests::catalogue();
        let mut indexed = FrameBounds::new(&input, &skills).unwrap();
        let mut exhaustive = FrameBounds::new(&input, &skills).unwrap();
        indexed.indexed = true;
        exhaustive.indexed = false;
        for correction in 0..=3 {
            for short in [1, 5, 9, 19] {
                for long in [short, short + 3, short + 40] {
                    let a = indexed.support(short, long, correction, &mut Default::default());
                    let b = exhaustive.support(short, long, correction, &mut Default::default());
                    for slot in 0..6 {
                        for delta in [-1., -0.03, 0., 0.01, 1.3] {
                            let coefficients = [[delta, delta]; 4];
                            assert_eq!(envelope(&a[slot], coefficients).map(f64::to_bits),
                                envelope(&b[slot], coefficients).map(f64::to_bits));
                        }
                    }
                }
            }
        }
    }
}

use crate::timing::Instant;
