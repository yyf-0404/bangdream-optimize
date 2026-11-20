//! Share exact arithmetic and repeated alternatives across skill orders.
use super::*;

type Sum = Vec<(usize, u32)>;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingOrderStats {
    pub complete: bool,
    pub orders: usize,
    pub separate_score_terms: usize,
    pub unique_score_terms: usize,
    pub unique_sums: usize,
    pub unique_choices: usize,
    pub term_additions_per_score: usize,
}

/// Joint evaluation of prepared candidate families, without assuming that a
/// player can predict future random activations. This returns separate scores;
/// an objective must define its own information policy before aggregating them.
pub struct CompiledTimingOrders {
    candidates: Vec<CompiledTimingCandidates>,
    terms: Vec<Term>,
    sums: Vec<Sum>,
    choices: Vec<Vec<usize>>,
    plans: Vec<(usize, Vec<usize>)>,
    stats: TimingOrderStats,
}

#[derive(Default)]
pub struct TimingOrderScratch {
    terms: Vec<u32>,
    sums: Vec<u64>,
    choices: Vec<u64>,
    scores: Vec<u64>,
}

fn intern<T: Clone + std::hash::Hash + Eq>(
    value: T,
    values: &mut Vec<T>,
    ids: &mut HashMap<T, usize>,
) -> usize {
    *ids.entry(value.clone()).or_insert_with(|| {
        let id = values.len();
        values.push(value);
        id
    })
}

impl CompiledTimingOrders {
    pub fn new(candidates: Vec<CompiledTimingCandidates>) -> Result<Self, SearchError> {
        let Some(first) = candidates.first() else {
            return Err(SearchError::Options(
                "at least one prepared order is required",
            ));
        };
        // These are the only global parameters used by NoteScorer. Each bank
        // already has its own validated geometry, states and witness frames.
        if candidates.iter().any(|c| {
            c.input.play_level != first.input.play_level
                || c.input.notes.len() != first.input.notes.len()
        }) {
            return Err(SearchError::Options(
                "orders need the same scoring chart dimensions",
            ));
        }
        let mut terms = Vec::new();
        let mut term_ids = HashMap::new();
        let mut sums = Vec::new();
        let mut sum_ids = HashMap::new();
        let mut choices = Vec::new();
        let mut choice_ids = HashMap::new();
        let mut plans = Vec::new();
        for c in &candidates {
            let remap: Vec<_> = c
                .terms
                .iter()
                .map(|&t| intern(t, &mut terms, &mut term_ids))
                .collect();
            let mut add_sum = |sum: &Sum| {
                let mut sum: Sum = sum.iter().map(|&(id, n)| (remap[id], n)).collect();
                sum.sort_unstable();
                intern(sum, &mut sums, &mut sum_ids)
            };
            let common = add_sum(&c.common);
            let mut selected = Vec::new();
            for group in &c.groups {
                if group.variants.len() == 1 {
                    continue;
                }
                let mut alternatives: Vec<_> =
                    group.variants.iter().map(|v| add_sum(&v.terms)).collect();
                alternatives.sort_unstable();
                alternatives.dedup();
                selected.push(intern(alternatives, &mut choices, &mut choice_ids));
            }
            plans.push((common, selected));
        }
        let stats = TimingOrderStats {
            complete: candidates.iter().all(|c| c.stats.complete),
            orders: candidates.len(),
            separate_score_terms: candidates.iter().map(|c| c.terms.len()).sum(),
            unique_score_terms: terms.len(),
            unique_sums: sums.len(),
            unique_choices: choices.len(),
            term_additions_per_score: sums.iter().map(Vec::len).sum(),
        };
        Ok(Self {
            candidates,
            terms,
            sums,
            choices,
            plans,
            stats,
        })
    }

    pub fn stats(&self) -> &TimingOrderStats {
        &self.stats
    }

    /// Scores retain the constructor's order. Complete banks solve the joint DP
    /// at this power; legacy-only batches share their finite-family arithmetic.
    pub fn score<'a>(
        &self,
        power: ScoringPower,
        scratch: &'a mut TimingOrderScratch,
    ) -> Result<&'a [u64], SearchError> {
        if self.candidates.iter().any(|c| c.exact.is_some()) {
            scratch.scores.clear();
            let mut query = TimingScoreScratch::default();
            for candidate in &self.candidates {
                scratch.scores.push(candidate.score(power, &mut query)?);
            }
            return Ok(&scratch.scores);
        }
        let input = &self.candidates[0].input;
        let scorer = NoteScorer::new(power, input.play_level, input.notes.len() as u32)?;
        scratch.terms.resize(self.terms.len(), 0);
        scratch.sums.resize(self.sums.len(), 0);
        scratch.choices.resize(self.choices.len(), 0);
        scratch.scores.resize(self.plans.len(), 0);
        for (t, value) in self.terms.iter().zip(&mut scratch.terms) {
            *value = scorer
                .score_with_judgement_multiplier(
                    f32::from_bits(t[0]),
                    f32::from_bits(t[1]),
                    f32::from_bits(t[2]),
                    1.0,
                )?
                .final_score;
        }
        for (sum, value) in self.sums.iter().zip(&mut scratch.sums) {
            *value = sum
                .iter()
                .map(|&(id, n)| u64::from(scratch.terms[id]) * u64::from(n))
                .sum();
        }
        for (choice, value) in self.choices.iter().zip(&mut scratch.choices) {
            *value = choice.iter().map(|&id| scratch.sums[id]).max().unwrap();
        }
        for ((common, choices), value) in self.plans.iter().zip(&mut scratch.scores) {
            *value = choices
                .iter()
                .try_fold(scratch.sums[*common], |total, &id| {
                    total
                        .checked_add(scratch.choices[id])
                        .ok_or(ScoringError::TotalScoreOverflow)
                })?;
        }
        Ok(&scratch.scores)
    }

    /// Slow reconstruction is intentionally outside the team enumeration loop.
    pub fn materialize(
        &self,
        order: usize,
        power: ScoringPower,
    ) -> Result<ReplayResult, SearchError> {
        self.candidates
            .get(order)
            .ok_or(SearchError::Options("prepared order index out of range"))?
            .materialize(power)
    }
}
