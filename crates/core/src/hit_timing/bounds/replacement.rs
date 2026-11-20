//! Experimental replacement bounds. `new` uses independent windows;
//! `new_adjacent` also builds the separate replay-witness continuation proof.
//! The fast same-input proof rejects durations that could move a successor;
//! `replanned` permits different inputs and advances both queued clocks.
//! Correction-changing proofs additionally preserve legal raw judgements and
//! account for changed grade multipliers and skill histories. General same-input
//! proofs do not imply closure of a heuristic's sampled family. Default scoring
//! now searches the full joint frame domain; limited/legacy candidate families
//! still cannot establish global replacement correctness.
use super::{cache::Geometry, countdown, *};
use crate::{hit_timing::ReplaySkill, scoring::linear_power_upper};
pub(super) mod frames;
mod replanned;
mod frame_cache;
pub use frame_cache::Stats as TimingFrameCacheStats;
type SharedFrameCoefficients = std::sync::Arc<std::sync::Mutex<frame_cache::Cache>>;

fn product_lower(a: [f64; 2], b: [f64; 2]) -> f64 {
    if a.iter().chain(&b).all(|v| v.is_finite()) && a[0] <= a[1] && b[0] <= b[1] {
        // Rounded multiplication is monotone on each sign quadrant. Compute
        // only the minimizing corner(s), then the very same downward neighbor.
        let product = if a[0] >= 0. {
            if b[0] >= 0. { a[0]*b[0] } else { a[1]*b[0] }
        } else if a[1] <= 0. {
            if b[1] <= 0. { a[1]*b[1] } else { a[0]*b[1] }
        } else if b[0] >= 0. { a[0]*b[1] }
        else if b[1] <= 0. { a[1]*b[0] }
        else { (a[0]*b[1]).min(a[1]*b[0]) };
        return product.next_down();
    }
    a.into_iter()
        .flat_map(|x| b.into_iter().map(move |y| (x * y).next_down()))
        .fold(f64::INFINITY, f64::min)
}

// DeltaP * (X + gB) + (Q + pA) * (gA-gB), equivalent to the
// old affine formula after cancelling Q*X. Also intersect with the lower
// certificate obtained from absolute coefficient intervals. Both certificates
// cover the SAME rectangle; no upper-minus-upper comparison or graph chaining.
fn affine_margin(
    pa: f64,
    pb: f64,
    q: [f64; 2],
    x: [f64; 2],
    old_total: [f64; 2],
    a: [f64; 2],
    b: [f64; 2],
    delta: f64,
) -> f64 {
    let dp = [(pa - pb).next_down(), (pa - pb).next_up()];
    let new_power = [(q[0] + pa).next_down(), (q[1] + pa).next_up()];
    let paired =
        (product_lower(dp, old_total) + product_lower(new_power, [delta, delta])).next_down();
    let power_term = product_lower(dp, x);
    let absolute = q
        .into_iter()
        .map(|v| {
            let term_a = product_lower([(v + pa).next_down(), (v + pa).next_up()], [a[0], a[0]]);
            let term_b = product_lower([(v + pb).next_down(), (v + pb).next_up()], [-b[1], -b[1]]);
            ((power_term + term_a).next_down() + term_b).next_down()
        })
        .fold(f64::INFINITY, f64::min);
    paired.max(absolute)
}

#[derive(Default, Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingReplacementStats {
    pub replanned: replanned::Stats,
    pub successor_changing_pairs: usize,
    pub model: &'static str,
    pub duration_profiles: usize,
    pub prefix_entries: usize,
    pub duration_queries: usize,
    pub compared_skill_pairs: usize,
    pub duration_pair_profiles: usize,
    pub tail_prefix_entries: usize,
    pub tail_queries: usize,
    pub frame_build_seconds: f64,
    pub frame_query_seconds: f64,
    pub frame_support_profiles: usize,
    pub frame_support_hits: usize,
    pub frame_start_rows: usize,
    pub frame_note_groups: usize,
    pub frame_pair_queries: usize,
    pub frame_coefficient_hits: u64,
}

#[derive(Clone, Copy, Default)]
struct TailWeights {
    possible: [f64; 6],
    guaranteed: [[f64; 2]; 6],
}

#[derive(Clone, Copy)]
struct ExtendedMargin {
    coefficient: f64,
    slots: [f64; 6],
    pointwise: bool,
}

/// Fixed conditional domain, excluding the replaced character. The ten rows
/// are first-five slots, then those same slots with the captain's encore.
/// Coefficients contain only the FOUR teammates' independent skill additions.
pub struct TimingAffineContext {
    pub other_power: [f64; 2],
    pub other_coefficient: [[f64; 2]; 10],
    pub required_score: f64,
}

#[derive(Clone)]
struct Profile {
    playing: u32,
    correction: u32,
    // Perfect-count before current note; then [clean G, clean P, spoiled G, spoiled P].
    states: Vec<[f32; 4]>,
    distinct_prefix: usize,
    maximum: f32,
}
impl Profile {
    fn new(playing: u32, correction: u32, states: Vec<[f32; 4]>) -> Self {
        let distinct_prefix = states.windows(2).rposition(|w|
            w[0].map(f32::to_bits) != w[1].map(f32::to_bits)).map_or(1, |i| i + 2);
        let maximum = states.iter().flat_map(|r| r.iter()).copied().fold(1_f32, f32::max);
        Self { playing, correction, states, distinct_prefix, maximum }
    }
    fn rows(&self) -> &[[f32; 4]] { &self.states[..self.distinct_prefix] }
    fn paired_rows<'a>(&'a self, other: &'a Self)
        -> impl Iterator<Item = (&'a [f32; 4], &'a [f32; 4])> {
        // Only the permanent repeated tail is omitted. Equal count indices
        // remain paired; a changing row of either profile is always retained.
        self.states.iter().zip(&other.states)
            .take(self.distinct_prefix.max(other.distinct_prefix))
    }
}

pub struct TimingReplacementBounds {
    shared_replanned: Option<replanned::SharedDifferences>,
    adjacent: bool,
    successors: [(u32, u32); 5],
    profiles: Vec<Profile>,
    duration_weights: BTreeMap<u32, [f64; 6]>,
    guaranteed_weights: BTreeMap<u32, [[f64; 2]; 6]>,
    losses: HashMap<(usize, usize), Option<f64>>,
    margins: HashMap<(usize, usize), Option<f64>>,
    slot_margins: HashMap<(usize, usize), Option<[f64; 6]>>,
    tails: BTreeMap<(u32, u32), TailWeights>,
    extended_margins: HashMap<(usize, usize), Option<ExtendedMargin>>,
    correction_margins: HashMap<(usize, usize), Option<[f64; 6]>>,
    power_gain: f64,
    base_upper: f64,
    base_lower: f64,
    coefficients: Vec<[[f64; 2]; 10]>,
    frames: Option<frames::FrameBounds>,
    replanned: Option<replanned::Replanned>,
    power_error: f64,
    score_error: f64,
    max_power: f64,
    stats: TimingReplacementStats,
}

impl TimingReplacementBounds {
    /// Diagnostic only: which structural restriction can prevent the existing
    /// same-input proof? `comparable` does not promise a nonnegative margin.
    pub fn comparison_class(&self, new: usize, old: usize) -> &'static str {
        let a = &self.profiles[new];
        let b = &self.profiles[old];
        if a.correction != b.correction {
            "differentCorrection"
        } else if a.playing == b.playing {
            "sameDuration"
        } else if a.correction != 0 {
            "correctedDurationChange"
        } else if self.adjacent
            && self
                .successors
                .iter()
                .any(|&(last, next)| last + a.playing.max(b.playing) + countdown(0.75) + 2 > next)
        {
            "successorChange"
        } else {
            "localDurationChange"
        }
    }

    /// Catalogue and power cap must cover every permitted team and condition.
    pub fn new(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        Self::build(input, skills, max_power, false)
    }
    pub fn new_adjacent(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
    ) -> Result<Self, ReplayError> {
        Self::build(input, skills, max_power, true)
    }
    fn build(
        input: &ReplayInput,
        skills: &[ReplaySkill],
        max_power: ScoringPower,
        adjacent: bool,
    ) -> Result<Self, ReplayError> {
        replay(input)?;
        if skills.is_empty() || input.skills.len() != 6 {
            return Err(ReplayError::Input(
                "replacement bound needs a catalogue and six requests",
            ));
        }
        let mut profiles = Vec::new();
        for (i, skill) in skills.iter().enumerate() {
            skill.validate(i)?;
            let mut state = SkillState::new(skill)?;
            let mut states = Vec::with_capacity(input.notes.len() + 1);
            for _ in 0..=input.notes.len() {
                let mut spoiled = state.clone();
                spoiled.multiplier(Judgement::Great);
                states.push([
                    state.clone().multiplier(Judgement::Great),
                    state.clone().multiplier(Judgement::Perfect),
                    spoiled.clone().multiplier(Judgement::Great),
                    spoiled.multiplier(Judgement::Perfect),
                ]);
                state.multiplier(Judgement::Perfect);
            }
            profiles.push(Profile::new(countdown(skill.duration) + 1,
                skill.cache_key()[1], states));
        }
        let strongest = skills.iter().max_by_key(|s| s.cache_key()[1]).unwrap();
        let mut widened = input.clone();
        widened.skills = vec![strongest.clone(); 6];
        let geometry = Geometry::new(&widened)?;
        let ideal = geometry.ideal_factors();
        let mut triggers = [None; 6];
        for (i, n) in input.notes.iter().enumerate() {
            if let Some(s) = n.trigger {
                triggers[s] = Some(i);
            }
        }
        if triggers.iter().any(Option::is_none) {
            return Err(ReplayError::Input(
                "replacement bound needs all six requests",
            ));
        }
        let mut windows: [(u32, u32); 6] = std::array::from_fn(|s| {
            let (first, last) = geometry.spans[triggers[s].unwrap()];
            (first + 1, last)
        });
        if adjacent {
            let shortest = profiles.iter().map(|p| p.playing).min().unwrap();
            let longest = profiles.iter().map(|p| p.playing).max().unwrap();
            let cooling = countdown(0.75) + 1;
            for s in 1..6 {
                windows[s].0 = windows[s].0.max(windows[s - 1].0 + shortest + cooling + 1);
                windows[s].1 = windows[s].1.max(windows[s - 1].1 + longest + cooling + 1);
            }
        }
        let successors =
            std::array::from_fn(|s| (windows[s].1, geometry.spans[triggers[s + 1].unwrap()].0));
        // Universal coefficient LOWER, not a playable baseline. Includes zero/
        // half penalties wherever any allowed skill could reach a note.
        let min_combo = (1..=input.notes.len() as u32)
            .map(|n| input.combo_multiplier(input.initial_combo + n))
            .fold(f32::INFINITY, f32::min);
        let minima: Vec<f32> = profiles
            .iter()
            .map(|p| {
                p.states
                    .iter()
                    .flat_map(|r| r.iter())
                    .copied()
                    .fold(1., f32::min)
            })
            .collect();
        let mut low = Vec::new();
        for (i, &(a, b)) in geometry.spans.iter().enumerate() {
            let mut skill = 1.0_f32;
            for (p, &m) in profiles.iter().zip(&minima) {
                if windows.iter().enumerate().any(|(s, &(first, last))| {
                    b >= first
                        && a <= last + p.playing
                        && input.notes[i].trigger.is_none_or(|t| t > s)
                }) {
                    skill = skill.min(m);
                }
            }
            low.push(NoteFactors {
                judgement: if matches!(input.notes[i].window, NoteWindow::PinnedPerfect { .. }) {
                    Judgement::Perfect.multiplier()
                } else {
                    Judgement::Great.multiplier()
                },
                combo: min_combo,
                fever: 1.,
                skill,
            });
        }
        let power_gain = CompiledScore::new(input.play_level, &low)?
            .envelope(max_power)?
            .coefficient_bounds()[0];
        let max_skill = profiles
            .iter()
            .flat_map(|p| p.states.iter())
            .flat_map(|r| r.iter())
            .copied()
            .fold(1., f32::max);
        let high: Vec<_> = ideal
            .iter()
            .map(|n| NoteFactors {
                skill: max_skill,
                ..*n
            })
            .collect();
        let envelope = CompiledScore::new(input.play_level, &high)?.envelope(max_power)?;
        let base_upper = CompiledScore::new(input.play_level, &ideal)?
            .envelope(max_power)?
            .coefficient_bounds()[1];
        let baseline: Vec<_> = low
            .iter()
            .map(|n| NoteFactors { skill: 1., ..*n })
            .collect();
        let base_lower = CompiledScore::new(input.play_level, &baseline)?
            .envelope(max_power)?
            .coefficient_bounds()[0];
        let score_error =
            ((2. * envelope.rounding_error()).next_up() + envelope.truncation_loss()).next_up();
        let max_power = f64::from(max_power);
        // Each team's actual native sum is within this full-pool envelope.
        let power_error = (2. * (linear_power_upper(max_power) - max_power).next_up()).next_up();

        // A single prefix per slot serves ALL durations; neighboring durations
        // only change the last queried entry. No per-duration note rescan.
        let rate = 1.0_f32 + (input.play_level - 5) as f32 * 0.01_f32;
        let base =
            ((f64::from(rate) / f64::from(input.notes.len() as f32)).next_up() * 3.).next_up();
        let weights: Vec<_> = ideal
            .iter()
            .map(|n| {
                (((base * f64::from(n.judgement)).next_up() * f64::from(n.combo)).next_up()
                    * f64::from(n.fever))
                .next_up()
            })
            .collect();
        let mut stats = TimingReplacementStats {
            model: if adjacent {
                "adjacent-skill-windows-v1"
            } else {
                "independent-skill-windows-v1"
            },
            ..TimingReplacementStats::default()
        };
        let mut duration_weights: BTreeMap<u32, [f64; 6]> =
            profiles.iter().map(|p| (p.playing, [0.; 6])).collect();
        let mut guaranteed_weights: BTreeMap<u32, [[f64; 2]; 6]> =
            profiles.iter().map(|p| (p.playing, [[0.; 2]; 6])).collect();
        let durations: Vec<_> = duration_weights.keys().copied().collect();
        let mut tails: BTreeMap<_, TailWeights> = durations
            .iter()
            .enumerate()
            .flat_map(|(i, &short)| {
                durations[i + 1..]
                    .iter()
                    .map(move |&long| ((short, long), TailWeights::default()))
            })
            .collect();
        for (s, &(first, last)) in windows.iter().enumerate() {
            let mut entries: Vec<_> = geometry
                .spans
                .iter()
                .enumerate()
                .filter(|&(i, &(_, b))| b >= first && input.notes[i].trigger.is_none_or(|t| t > s))
                .map(|(i, &(a, _))| (a, weights[i]))
                .collect();
            entries.sort_by_key(|e| e.0);
            let mut prefix = vec![0.];
            for &(_, w) in &entries {
                prefix.push((prefix.last().unwrap() + w).next_up());
            }
            stats.prefix_entries += entries.len();
            for (&playing, values) in &mut duration_weights {
                values[s] = prefix[entries.partition_point(|e| e.0 <= last + playing)];
                stats.duration_queries += 1;
            }
            // Independent-window LOWER: even the latest allowed request starts
            // before the earliest hit, and the earliest request ends after the
            // latest hit. No other skill's duration/request modifies this range.
            let base_low = ((f64::from(rate) / f64::from(input.notes.len() as f32)).next_down()
                * 3.)
                .next_down();
            let mut guaranteed: Vec<_> = geometry
                .spans
                .iter()
                .enumerate()
                .filter(|&(i, &(a, _))| a > last && input.notes[i].trigger.is_none_or(|t| t > s))
                .map(|(i, &(_, b))| {
                    let pinned = matches!(input.notes[i].window, NoteWindow::PinnedPerfect { .. });
                    let grade = if pinned { 1.1_f32 } else { 0.8_f32 };
                    let weight = ((base_low * f64::from(grade)).next_down() * f64::from(min_combo))
                        .next_down();
                    (b, weight, pinned)
                })
                .collect();
            guaranteed.sort_by_key(|e| e.0);
            let mut prefix = vec![[0_f64; 2]];
            for &(_, weight, pinned) in &guaranteed {
                let prior = *prefix.last().unwrap();
                prefix.push([
                    (prior[0] + weight).next_down().max(0.),
                    if pinned {
                        (prior[1] + weight).next_down().max(0.)
                    } else {
                        prior[1]
                    },
                ]);
            }
            for (&playing, values) in &mut guaranteed_weights {
                values[s] = prefix[guaranteed.partition_point(|e| e.0 <= first - 1 + playing)];
            }
            // For each shorter duration build two prefixes once. All longer
            // durations reuse them: no skill-pair or neighboring-slot scan.
            for (d, &short) in durations
                .iter()
                .enumerate()
                .take(durations.len().saturating_sub(1))
            {
                let mut possible_tail: Vec<_> = geometry
                    .spans
                    .iter()
                    .enumerate()
                    .filter(|&(i, &(_, b))| {
                        b > first - 1 + short && input.notes[i].trigger.is_none_or(|t| t > s)
                    })
                    .map(|(i, &(a, _))| (a, weights[i]))
                    .collect();
                possible_tail.sort_by_key(|e| e.0);
                let mut upper_prefix = vec![0_f64];
                for &(_, weight) in &possible_tail {
                    upper_prefix.push((upper_prefix.last().unwrap() + weight).next_up());
                }
                let mut certain_tail: Vec<_> = geometry
                    .spans
                    .iter()
                    .enumerate()
                    .filter(|&(i, &(a, _))| {
                        a > last + short && input.notes[i].trigger.is_none_or(|t| t > s)
                    })
                    .map(|(i, &(_, b))| {
                        let pinned =
                            matches!(input.notes[i].window, NoteWindow::PinnedPerfect { .. });
                        let grade = if pinned { 1.1_f32 } else { 0.8_f32 };
                        (
                            b,
                            ((base_low * f64::from(grade)).next_down() * f64::from(min_combo))
                                .next_down(),
                            pinned,
                        )
                    })
                    .collect();
                certain_tail.sort_by_key(|e| e.0);
                let mut lower_prefix = vec![[0_f64; 2]];
                for &(_, weight, pinned) in &certain_tail {
                    let prior = *lower_prefix.last().unwrap();
                    lower_prefix.push([
                        (prior[0] + weight).next_down().max(0.),
                        if pinned {
                            (prior[1] + weight).next_down().max(0.)
                        } else {
                            prior[1]
                        },
                    ]);
                }
                stats.tail_prefix_entries += possible_tail.len() + certain_tail.len();
                for &long in &durations[d + 1..] {
                    let tail = tails.get_mut(&(short, long)).unwrap();
                    tail.possible[s] =
                        upper_prefix[possible_tail.partition_point(|e| e.0 <= last + long)];
                    tail.guaranteed[s] =
                        lower_prefix[certain_tail.partition_point(|e| e.0 <= first - 1 + long)];
                    stats.tail_queries += 2;
                }
            }
        }
        stats.duration_profiles = duration_weights.len();
        stats.duration_pair_profiles = tails.len();
        let coefficients = profiles
            .iter()
            .map(|p| {
                let mut minimum = f64::INFINITY;
                let mut maximum = f64::NEG_INFINITY;
                for row in &p.states {
                    for (k, &m) in row.iter().enumerate() {
                        if p.correction != 0 && k != 1 {
                            continue;
                        }
                        minimum = minimum.min(f64::from(m) - 1.);
                        maximum = maximum.max(f64::from(m) - 1.);
                    }
                }
                let slots: [[f64; 2]; 6] = std::array::from_fn(|s| {
                    let possible = duration_weights[&p.playing][s];
                    let certain = guaranteed_weights[&p.playing][s][0];
                    [
                        (minimum.next_down() * if minimum >= 0. { certain } else { possible })
                            .next_down(),
                        (maximum.next_up() * if maximum >= 0. { possible } else { certain })
                            .next_up(),
                    ]
                });
                std::array::from_fn(|s| {
                    if s < 5 {
                        slots[s]
                    } else {
                        [
                            (slots[s - 5][0] + slots[5][0]).next_down(),
                            (slots[s - 5][1] + slots[5][1]).next_up(),
                        ]
                    }
                })
            })
            .collect();
        Ok(Self {
            shared_replanned: None,
            adjacent,
            successors,
            profiles,
            duration_weights,
            guaranteed_weights,
            losses: HashMap::new(),
            margins: HashMap::new(),
            slot_margins: HashMap::new(),
            tails,
            extended_margins: HashMap::new(),
            correction_margins: HashMap::new(),
            power_gain,
            base_upper,
            base_lower,
            coefficients,
            frames: None,
            replanned: None,
            power_error,
            score_error,
            max_power,
            stats,
        })
    }

    pub fn stats(&self) -> &TimingReplacementStats {
        &self.stats
    }

    /// Opt-in frame refinement, after global v1 pruning. Preserve every old
    /// valid envelope by intersection; no change to executable scoring.
    pub fn tighten_frames(
        &mut self,
        input: &ReplayInput,
        skills: &[ReplaySkill],
    ) -> Result<(), ReplayError> {
        self.tighten_frames_shared(input, skills, None)
    }
    fn tighten_frames_shared(&mut self, input: &ReplayInput, skills: &[ReplaySkill],
        shared: Option<SharedFrameCoefficients>) -> Result<(), ReplayError> {
        if self.frames.is_some() {
            return Ok(());
        }
        let start = crate::timing::Instant::now();
        let mut frames = frames::FrameBounds::new(input, skills)?;
        frames.shared = shared;
        if self.adjacent {
            frames.delay_requests(skills);
        }
        self.base_lower = self.base_lower.max(frames.base[0]);
        self.base_upper = self.base_upper.min(frames.base[1]);
        for (i, p) in self.profiles.iter().enumerate() {
            let rows = frames.profile(i, p, &mut self.stats);
            for (existing, new) in self.coefficients[i].iter_mut().zip(rows) {
                existing[0] = existing[0].max(new[0]);
                existing[1] = existing[1].min(new[1]);
                if existing[0] > existing[1] {
                    return Err(ReplayError::Input(
                        "inconsistent frame contribution interval",
                    ));
                }
            }
        }
        self.stats.frame_build_seconds += start.elapsed().as_secs_f64();
        if self.adjacent {
            let mut model = replanned::Replanned::new(input, skills, &frames)?;
            model.shared = self.shared_replanned.clone();
            model.shared_rows = frames.shared.clone();
            self.replanned = Some(model);
        }
        self.frames = Some(frames);
        Ok(())
    }

    /// Fix the successor catalogue for an entire proof/graph stage. It must
    /// include every teammate shape possible in this stage, before deletions.
    pub fn set_replanned_catalogue(&mut self, ids: &[usize]) {
        if let Some(model) = &mut self.replanned {
            model.set_catalogue(ids);
        }
    }

    /// User per-note limits are preserved by same-frame substitution proofs.
    /// A replanned witness can move hits outside those limits, so it cannot
    /// certify card removal in a restricted search.
    pub fn require_same_hit_frames(mut self) -> Self {
        self.replanned = None;
        self
    }

    /// A different, fully legal input plan may witness the replacement. The
    /// old side is an upper envelope of ALL inputs, never another lower bound.
    pub fn can_replace_replanned_context(
        &mut self,
        new: usize,
        old: usize,
        new_power: crate::scoring::MemberPower,
        old_power: crate::scoring::MemberPower,
        other_power: [f64; 2],
        coefficient_upper: f64,
        required_score: f64,
    ) -> Result<bool, ReplayError> {
        if new >= self.profiles.len() || old >= self.profiles.len() {
            return Err(ReplayError::Input("unknown replacement skill"));
        }
        let Some(model) = &mut self.replanned else {
            return Ok(false);
        };
        // This path also permits different corrections. Its witness uses raw
        // Perfect inputs, so no old corrected Good/Bad frame has to be copied.
        let start = crate::timing::Instant::now();
        let delta = model.difference(new, old, &mut self.stats.replanned);
        let witness_floor = model.witness_floor();
        self.stats.replanned.seconds += start.elapsed().as_secs_f64();
        Ok(delta.is_some_and(|d| {
            self.context_margin(
                d,
                new_power,
                old_power,
                other_power,
                coefficient_upper,
                required_score,
            ) || {
                // Equivalent decomposition using the NEW coefficient:
                // (PA-PB)*Cnew + PB*(Cnew-Cold). Its legal Perfect witness
                // gives a stronger floor than arbitrary old zero-score hits.
                let dp = (new_power.linear_total() - old_power.linear_total()).next_down();
                let dp = (dp - self.power_error).next_down();
                let power_term = (dp
                    * if dp >= 0. {
                        witness_floor
                    } else {
                        coefficient_upper
                    })
                .next_down();
                let old_floor = ((required_score - self.score_error).next_down()
                    / coefficient_upper)
                    .next_down()
                    .max(0.);
                let low = ((other_power[0] + old_power.linear_total())
                    .next_down()
                    .max(old_floor)
                    - self.power_error)
                    .next_down();
                let high = ((other_power[1] + old_power.linear_total()).next_up()
                    + self.power_error)
                    .next_up();
                let skill_term = (d * if d >= 0. { low.max(0.) } else { high }).next_down();
                (power_term + skill_term).next_down() > self.score_error
            }
        }))
    }

    pub fn different_duration(&self, a: usize, b: usize) -> bool {
        self.profiles[a].playing != self.profiles[b].playing
    }

    /// Reuse the very same hit frames across different judgement corrections.
    /// Equal playing lengths preserve every queued activation and note owner.
    /// An old Good/Bad correction may only be replaced by an equally permissive
    /// correction. Dropping Great->Perfect is legal (Great is still accepted),
    /// but the certificate must pay for the lost judgement multiplier as well
    /// as changes to skill history. Frame rows bound TOTAL score differences;
    /// they must not be used as independent skill-addition/affine coefficients.
    pub fn can_replace_correction_context(
        &mut self,
        new: usize,
        old: usize,
        new_power: crate::scoring::MemberPower,
        old_power: crate::scoring::MemberPower,
        other_power: [f64; 2],
        coefficient_upper: f64,
        required_score: f64,
    ) -> Result<bool, ReplayError> {
        let a = self
            .profiles
            .get(new)
            .ok_or(ReplayError::Input("unknown replacement skill"))?;
        let b = self
            .profiles
            .get(old)
            .ok_or(ReplayError::Input("unknown replacement skill"))?;
        if a.playing != b.playing
            || a.correction == b.correction
            || (b.correction > 1 && a.correction < b.correction)
        {
            return Ok(false);
        }
        let Some(frames) = &mut self.frames else {
            return Ok(false);
        };
        let slots = *self.correction_margins.entry((new, old)).or_insert_with(|| {
            let key = [1,new,old];
            if let Some(rows) = frames.shared.as_ref().and_then(|c| c.lock().unwrap().pair(key)) {
                self.stats.frame_coefficient_hits += 1;
                return Some(rows);
            }
            let rows = frames.correction_difference(a, b, &mut self.stats);
            if let Some(cache) = &frames.shared { cache.lock().unwrap().store_pair(key,rows); }
            Some(rows)
        });
        let slots = slots.expect("eligible correction pair");
        let delta = (slots[..5].iter().copied().fold(f64::INFINITY, f64::min) + slots[5].min(0.))
            .next_down();
        Ok(self.context_margin(
            delta,
            new_power,
            old_power,
            other_power,
            coefficient_upper,
            required_score,
        ))
    }

    /// Upper coefficient for a signature's catalogue. Independent contributions
    /// are added to the non-skill baseline; no neighboring-slot propagation.
    pub fn coefficient_upper(&self, ids: &[usize]) -> f64 {
        let mut result = self.base_upper;
        for slot in 0..6 {
            let addition = ids
                .iter()
                .map(|&id| {
                    let p = &self.profiles[id];
                    let multiplier = p.maximum;
                    (self.duration_weights[&p.playing][slot]
                        * (f64::from(multiplier) - 1.).next_up())
                    .next_up()
                })
                .fold(0_f64, f64::max);
            result = (result + addition).next_up();
        }
        result
    }

    /// Signed coefficient of new-old for one arbitrary slot, with an optional
    /// captain encore. This stronger experiment treats windows independently,
    /// including their contributions; unlike loss_coefficient it does not
    /// certify overwriting/queued overlap semantics.
    fn signed_coefficient(&mut self, new: usize, old: usize) -> Result<Option<f64>, ReplayError> {
        if let Some(&value) = self.margins.get(&(new, old)) {
            return Ok(value);
        }
        let slots = self.signed_slot_coefficients(new, old)?;
        let value = slots.map(|values| {
            (values[..5].iter().copied().fold(f64::INFINITY, f64::min) + values[5].min(0.))
                .next_down()
        });
        self.margins.insert((new, old), value);
        Ok(value)
    }

    fn signed_slot_coefficients(
        &mut self,
        new: usize,
        old: usize,
    ) -> Result<Option<[f64; 6]>, ReplayError> {
        if let Some(&value) = self.slot_margins.get(&(new, old)) {
            return Ok(value);
        }
        let a = self
            .profiles
            .get(new)
            .ok_or(ReplayError::Input("unknown replacement skill"))?;
        let b = self
            .profiles
            .get(old)
            .ok_or(ReplayError::Input("unknown replacement skill"))?;
        let value = if a.playing != b.playing || a.correction != b.correction {
            None
        } else {
            let mut general = f64::INFINITY;
            let mut perfect = f64::INFINITY;
            for (x, y) in a.paired_rows(b) {
                for k in 0..4 {
                    if a.correction != 0 && k != 1 {
                        continue;
                    }
                    let raw = f64::from(x[k]) - f64::from(y[k]);
                    let delta = if raw == 0. { 0. } else { raw.next_down() };
                    general = general.min(delta);
                    if k % 2 == 1 {
                        perfect = perfect.min(delta);
                    }
                }
            }
            let possible = self.duration_weights[&a.playing];
            let guaranteed = self.guaranteed_weights[&a.playing];
            let values: [f64; 6] = std::array::from_fn(|s| {
                if general < 0. {
                    (possible[s] * general).next_down()
                } else {
                    ((guaranteed[s][0] * general).next_down()
                        + (guaranteed[s][1] * (perfect - general).next_down().max(0.)).next_down())
                    .next_down()
                }
            });
            Some(values)
        };
        self.slot_margins.insert((new, old), value);
        Ok(value)
    }

    /// Both signs of power and skill changes. other_power bounds the FOUR
    /// other distinct characters. required_score is a necessary score floor
    /// for this song, derived from the incumbent and other-song upper bounds.
    pub fn can_replace_in_context(
        &mut self,
        new: usize,
        old: usize,
        new_power: crate::scoring::MemberPower,
        old_power: crate::scoring::MemberPower,
        other_power: [f64; 2],
        coefficient_upper: f64,
        required_score: f64,
    ) -> Result<bool, ReplayError> {
        if self.can_replace(new, old, new_power, old_power)? {
            return Ok(true);
        }
        let Some(skill_delta) = self.signed_coefficient(new, old)? else {
            return Ok(false);
        };
        Ok(self.context_margin(
            skill_delta,
            new_power,
            old_power,
            other_power,
            coefficient_upper,
            required_score,
        ))
    }

    /// Extends the independent model across durations. Correction-changing
    /// tails are excluded because keeping the same input must remain legal.
    pub fn can_replace_extended_context(
        &mut self,
        new: usize,
        old: usize,
        new_power: crate::scoring::MemberPower,
        old_power: crate::scoring::MemberPower,
        other_power: [f64; 2],
        coefficient_upper: f64,
        required_score: f64,
    ) -> Result<bool, ReplayError> {
        if self.can_replace_in_context(
            new,
            old,
            new_power,
            old_power,
            other_power,
            coefficient_upper,
            required_score,
        )? {
            return Ok(true);
        }
        let Some(margin) = self.extended_coefficient(new, old)? else {
            return Ok(false);
        };
        if margin.pointwise && new_power.componentwise_dominates(old_power) {
            return Ok(true);
        }
        Ok(self.context_margin(
            margin.coefficient,
            new_power,
            old_power,
            other_power,
            coefficient_upper,
            required_score,
        ))
    }

    fn extended_coefficient(
        &mut self,
        new: usize,
        old: usize,
    ) -> Result<Option<ExtendedMargin>, ReplayError> {
        if let Some(&value) = self.extended_margins.get(&(new, old)) {
            return Ok(value);
        }
        let a = self
            .profiles
            .get(new)
            .ok_or(ReplayError::Input("unknown replacement skill"))?;
        let b = self
            .profiles
            .get(old)
            .ok_or(ReplayError::Input("unknown replacement skill"))?;
        if a.playing == b.playing || a.correction != 0 || b.correction != 0 {
            self.extended_margins.insert((new, old), None);
            return Ok(None);
        }
        // A duration change is local only when every following request is
        // after BOTH cooldowns. Otherwise its effect includes a shifted suffix;
        // the independent common/tail certificate is not a proof for that pair.
        if self.adjacent
            && self
                .successors
                .iter()
                .any(|&(last, next)| last + a.playing.max(b.playing) + countdown(0.75) + 2 > next)
        {
            self.stats.successor_changing_pairs += 1;
            self.extended_margins.insert((new, old), None);
            return Ok(None);
        }
        let lower = |v: f64| if v == 0. { 0. } else { v.next_down() };
        let mut common = [f64::INFINITY; 2];
        let mut tail = [f64::INFINITY; 2];
        for (x, y) in a.paired_rows(b) {
            for k in 0..4 {
                let difference = lower(f64::from(x[k]) - f64::from(y[k]));
                let extension = lower(if a.playing > b.playing {
                    f64::from(x[k]) - 1.
                } else {
                    1. - f64::from(y[k])
                });
                common[0] = common[0].min(difference);
                tail[0] = tail[0].min(extension);
                if k % 2 == 1 {
                    common[1] = common[1].min(difference);
                    tail[1] = tail[1].min(extension);
                }
            }
        }
        let short = a.playing.min(b.playing);
        let long = a.playing.max(b.playing);
        let extra = self.tails[&(short, long)];
        let weighted = |delta: [f64; 2], upper: f64, low: [f64; 2]| {
            if delta[0] < 0. {
                (upper * delta[0]).next_down()
            } else {
                ((low[0] * delta[0]).next_down()
                    + (low[1] * (delta[1] - delta[0]).next_down().max(0.)).next_down())
                .next_down()
            }
        };
        let slots: [f64; 6] = std::array::from_fn(|s| {
            (weighted(
                common,
                self.duration_weights[&short][s],
                self.guaranteed_weights[&short][s],
            ) + weighted(tail, extra.possible[s], extra.guaranteed[s]))
            .next_down()
        });
        let result = ExtendedMargin {
            coefficient: (slots[..5].iter().copied().fold(f64::INFINITY, f64::min)
                + slots[5].min(0.))
            .next_down(),
            slots,
            pointwise: common[0] >= 0. && tail[0] >= 0.,
        };
        self.extended_margins.insert((new, old), Some(result));
        Ok(Some(result))
    }

    /// Reusable per-slot intervals; these are bounds, NOT playable maxima.
    pub fn scenario_coefficients(&self, id: usize) -> [[f64; 2]; 10] {
        self.coefficients[id]
    }

    /// Same independent-window model as v2/v3, with no queue/neighbor checks.
    /// Preserve all ten scenarios and the common teammate coefficient domain.
    /// A lower bound on the same-plan skill difference is used, never a
    /// subtraction of two optimistic skill upper bounds.
    pub fn can_replace_affine_context(
        &mut self,
        new: usize,
        old: usize,
        new_power: crate::scoring::MemberPower,
        old_power: crate::scoring::MemberPower,
        context: &TimingAffineContext,
        cross_duration: bool,
    ) -> Result<bool, ReplayError> {
        let mut slots = if let Some(slots) = self.signed_slot_coefficients(new, old)? {
            slots
        } else if cross_duration {
            let Some(value) = self.extended_coefficient(new, old)? else {
                return Ok(false);
            };
            value.slots
        } else {
            return Ok(false);
        };
        if let Some(frames) = &mut self.frames {
            let tighter = frames.difference(new, old, &self.profiles, &mut self.stats);
            for (old, new) in slots.iter_mut().zip(tighter) {
                *old = old.max(new);
            }
        }
        let pa = new_power.linear_total();
        let pb = old_power.linear_total();
        let a = self.coefficients[new];
        let b = self.coefficients[old];
        for s in 0..10 {
            let x = [
                (self.base_lower + context.other_coefficient[s][0]).next_down(),
                (self.base_upper + context.other_coefficient[s][1]).next_up(),
            ];
            let old_total = [(x[0] + b[s][0]).next_down(), (x[1] + b[s][1]).next_up()];
            let new_total = [(x[0] + a[s][0]).next_down(), (x[1] + a[s][1]).next_up()];
            let magnitude = old_total
                .into_iter()
                .chain(new_total)
                .map(f64::abs)
                .fold(0., f64::max);
            let error = (self.score_error + (self.power_error * magnitude).next_up()).next_up();
            let mut q = context.other_power;
            if old_total[1] > 0. {
                q[0] = q[0].max(
                    (((context.required_score - error).next_down() / old_total[1]).next_down()
                        - pb)
                        .next_down(),
                );
            }
            if q[0] > q[1] {
                continue;
            } // Old card cannot meet this scenario's floor.
            let delta = if s < 5 {
                slots[s]
            } else {
                (slots[s - 5] + slots[5]).next_down()
            };
            let mut total_low = old_total[0];
            if q[1] + pb > 0. {
                total_low = total_low.max(
                    ((context.required_score - error).next_down() / (q[1] + pb).next_up())
                        .next_down(),
                );
            }
            let margin = affine_margin(pa, pb, q, x, [total_low, old_total[1]], a[s], b[s], delta);
            if margin <= error {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn context_margin(
        &self,
        skill_delta: f64,
        new_power: crate::scoring::MemberPower,
        old_power: crate::scoring::MemberPower,
        other_power: [f64; 2],
        coefficient_upper: f64,
        required_score: f64,
    ) -> bool {
        let scalar_delta = new_power.linear_total() - old_power.linear_total();
        let delta = (scalar_delta.next_down() - self.power_error).next_down();
        let power_term = (delta
            * if delta >= 0. {
                self.power_gain
            } else {
                coefficient_upper
            })
        .next_down();
        let old_floor = (((required_score - self.score_error).next_down() / coefficient_upper)
            .next_down())
        .max(0.);
        let new_low = ((other_power[0] + new_power.linear_total())
            .next_down()
            .max((old_floor + scalar_delta).next_down())
            - self.power_error)
            .next_down();
        let new_high =
            ((other_power[1] + new_power.linear_total()).next_up() + self.power_error).next_up();
        let skill_term = (skill_delta
            * if skill_delta >= 0. {
                new_low.max(0.)
            } else {
                new_high
            })
        .next_down();
        (power_term + skill_term).next_down() > self.score_error
    }

    /// Upper coefficient of score lost when changing old -> new at the SAME
    /// power/plan. Covers each first-five slot and a possible captain encore.
    /// None: timing or correction differs, so this certificate does not apply.
    pub fn loss_coefficient(&mut self, new: usize, old: usize) -> Result<Option<f64>, ReplayError> {
        if let Some(&v) = self.losses.get(&(new, old)) {
            return Ok(v);
        }
        let a = self
            .profiles
            .get(new)
            .ok_or(ReplayError::Input("unknown replacement skill"))?;
        let b = self
            .profiles
            .get(old)
            .ok_or(ReplayError::Input("unknown replacement skill"))?;
        let result = if a.playing != b.playing || a.correction != b.correction {
            None
        } else {
            let mut loss = 0.0_f64;
            for (a_state, b_state) in a.paired_rows(b) {
                for k in 0..4 {
                    // Any nonempty correction turns every accepted note into P;
                    // no spoiled history is then reachable under this skill.
                    if a.correction != 0 && k != 1 {
                        continue;
                    }
                    loss = loss.max(f64::from(b_state[k]) - f64::from(a_state[k]));
                }
            }
            let weights = self.duration_weights[&a.playing];
            let weight = (weights[..5].iter().copied().fold(0., f64::max) + weights[5]).next_up();
            Some(if loss == 0. {
                0.
            } else {
                (weight * loss.next_up()).next_up()
            })
        };
        self.stats.compared_skill_pairs += 1;
        self.losses.insert((new, old), result);
        Ok(result)
    }

    /// Native pointwise monotonicity needs no rounding allowance. Otherwise
    /// DeltaP * minimum coefficient - Pmax * maximum loss must pay BOTH native
    /// float errors and the replacement's two integer-cast losses.
    pub fn can_replace(
        &mut self,
        new: usize,
        old: usize,
        new_power: crate::scoring::MemberPower,
        old_power: crate::scoring::MemberPower,
    ) -> Result<bool, ReplayError> {
        let Some(loss) = self.loss_coefficient(new, old)? else {
            return Ok(false);
        };
        if loss == 0. && new_power.componentwise_dominates(old_power) {
            return Ok(true);
        }
        let delta = ((new_power.linear_total() - old_power.linear_total()).next_down()
            - self.power_error)
            .next_down();
        if delta <= 0. {
            return Ok(false);
        }
        let gain = (delta * self.power_gain).next_down();
        let loss = ((self.max_power * loss).next_up() + self.score_error).next_up();
        Ok(gain > loss)
    }
}

#[cfg(test)]
mod tests;

/// Bound to one immutable chart, ordered skill catalogue and witness family.
/// Shares finished continuous differences and primitive frame/window
/// coefficients, never a Boolean replacement decision. Every prepare rebuilds
/// native errors at its own cap.
pub struct TimingReplacementCache {
    input: ReplayInput,
    skills: Vec<ReplaySkill>,
    shared: replanned::SharedDifferences,
    frames: SharedFrameCoefficients,
}
impl TimingReplacementCache {
    pub fn new(input: &ReplayInput, skills: &[ReplaySkill]) -> Self {
        Self {
            input: input.clone(),
            skills: skills.to_vec(),
            shared: Default::default(),
            frames: std::sync::Arc::new(std::sync::Mutex::new(frame_cache::Cache::new(skills.len()))),
        }
    }
    pub fn prepare(&self, cap: ScoringPower) -> Result<TimingReplacementBounds, ReplayError> {
        let mut model = TimingReplacementBounds::new_adjacent(&self.input, &self.skills, cap)?;
        model.shared_replanned = Some(self.shared.clone());
        Ok(model)
    }
    /// This owner supplies the same immutable input and complete catalogue to
    /// build and refinement. Public arbitrary-input refinement stays uncached.
    pub fn prepare_tightened(&self, cap: ScoringPower) -> Result<TimingReplacementBounds, ReplayError> {
        let mut model = self.prepare(cap)?;
        model.tighten_frames_shared(&self.input,&self.skills,Some(self.frames.clone()))?;
        Ok(model)
    }
    pub fn frame_cache_stats(&self) -> TimingFrameCacheStats { self.frames.lock().unwrap().stats.clone() }
}
