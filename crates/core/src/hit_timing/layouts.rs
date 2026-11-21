//! Exact request sharing across all physical layouts, captains and native orders.
//! Full resolved skill sequences and original f32 power bits are coalesced;
//! every physical layout retains its actual card order and native path weights.
use super::{search::SearchError, ReplayError, ReplaySkill};
use crate::{
    scoring::{ScoringError, ScoringPower},
    skill_shuffle::{tables, CAPTAIN_SLOT, SHUFFLE_PATH_COUNT},
};
use std::{borrow::Cow, collections::BTreeMap, sync::OnceLock};

pub fn physical_layouts() -> Vec<[usize; 5]> {
    tables().layouts.iter().map(|l| l.cards).collect()
}

#[derive(Clone, Debug)]
pub struct LayoutScoreRequest {
    /// Indices in the caller's unordered five-card roster, including encore.
    pub slots: [usize; 6],
    pub power: ScoringPower,
    pub configuration: usize,
}

pub struct TimingLayouts {
    requests: Vec<LayoutScoreRequest>,
    // Cards, native orders and path weights already live in tables(). Only the
    // roster/power-dependent request IDs vary. There are at most 120 * 96 IDs.
    references: Cow<'static, [u16]>,
    configurations: Vec<Vec<usize>>,
}

// Only the equality pattern of the five skills determines which sequences
// coincide. At most 52 patterns are reachable, irrespective of the inventory.
struct LayoutPattern {
    configurations: Vec<[usize; 6]>,
    references: Vec<u16>,
}

impl LayoutPattern {
    fn for_classes(classes: [usize; 5]) -> &'static Self {
        // Classes are numbered on first appearance, so classes[i] <= i.
        let index = classes.iter().enumerate().fold(0, |key, (i, class)| key * (i + 1) + class);
        static PATTERNS: [OnceLock<LayoutPattern>; 120] = [const { OnceLock::new() }; 120];
        PATTERNS[index].get_or_init(|| {
            let mut keys = BTreeMap::new();
            let mut configurations = Vec::new();
            let mut references = Vec::new();
            for layout in &tables().layouts {
                for order in &layout.weighted_orders {
                    let mut slots = [layout.cards[CAPTAIN_SLOT]; 6];
                    slots[..5].copy_from_slice(&tables().orders[order.index]);
                    let sequence = slots.map(|i| classes[i]);
                    let id = *keys.entry(sequence).or_insert_with(|| {
                        let id = configurations.len() as u16;
                        configurations.push(slots);
                        id
                    });
                    references.push(id);
                }
            }
            Self { configurations, references }
        })
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutMaximum {
    pub score: u64,
    pub recommended_layout: [usize; 5],
    pub chosen_request: usize,
    /// Actual card indices for the recommended layout, even when its numerical
    /// scoring request was shared with equal skills on different cards.
    pub chosen_slots: [usize; 6],
    /// Sum of tied per-order candidate maxima for this layout. Not a proof of
    /// one causal policy when timing plans depend on unknown future activations.
    pub max_score_path_count: u16,
    pub optimal_order_count: usize,
    pub total_path_count: u16,
    pub scored_requests: usize,
    pub possible_requests: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("one upper bound is required per distinct layout scoring request")]
    InvalidBounds,
    #[error("layout request {request}: actual {actual} exceeds upper {upper}")]
    Underestimated {
        request: usize,
        actual: u64,
        upper: u64,
    },
    #[error(transparent)]
    Search(#[from] SearchError),
}

impl TimingLayouts {
    /// Call the native power path in each physical order. Do not pass a display
    /// integer or one canonical power for all layouts: addition order matters.
    pub fn new(
        power: impl FnMut([usize; 5]) -> Result<ScoringPower, ScoringError>,
    ) -> Result<Self, ScoringError> {
        Self::build([0, 1, 2, 3, 4], power)
    }

    /// Resolve team conditions first. Equality includes duration, correction,
    /// effect kind and every effect operand; equal peak multipliers alone do not
    /// imply equal behavior when Great or stateful skills are allowed.
    pub fn for_skills(
        skills: &[ReplaySkill; 5],
        power: impl FnMut([usize; 5]) -> Result<ScoringPower, ScoringError>,
    ) -> Result<Self, ReplayError> {
        for (i, s) in skills.iter().enumerate() {
            s.validate(i)?;
        }
        let mut keys = BTreeMap::new();
        let classes = skills.each_ref().map(|s| {
            let next = keys.len();
            *keys.entry(s.cache_key()).or_insert(next)
        });
        Ok(Self::build(classes, power)?)
    }

    fn build(
        classes: [usize; 5],
        mut power: impl FnMut([usize; 5]) -> Result<ScoringPower, ScoringError>,
    ) -> Result<Self, ScoringError> {
        let tables = tables();
        let powers = tables.layouts.iter().map(|l| power(l.cards)).collect::<Result<Vec<_>, _>>()?;
        let pattern = LayoutPattern::for_classes(classes);
        if powers.iter().all(|p| *p == powers[0]) {
            let requests = pattern.configurations.iter().enumerate().map(|(configuration, &slots)| {
                LayoutScoreRequest { slots, power: powers[0], configuration }
            }).collect();
            return Ok(Self {
                requests,
                references: Cow::Borrowed(&pattern.references),
                configurations: (0..pattern.configurations.len()).map(|i| vec![i]).collect(),
            });
        }
        let mut by_power = BTreeMap::new();
        let mut configurations = vec![Vec::new(); pattern.configurations.len()];
        let mut requests = Vec::new();
        let mut references = Vec::with_capacity(pattern.references.len());
        for (layout, p) in tables.layouts.iter().zip(powers) {
            let row = by_power.entry(p.cache_key())
                .or_insert_with(|| vec![u16::MAX; configurations.len()]);
            for order in &layout.weighted_orders {
                let configuration = usize::from(pattern.references[references.len()]);
                let id = &mut row[configuration];
                if *id == u16::MAX {
                    // At most 120 layouts times 96 reachable orders.
                    *id = requests.len() as u16;
                    let mut slots = [layout.cards[CAPTAIN_SLOT]; 6];
                    slots[..5].copy_from_slice(&tables.orders[order.index]);
                    configurations[configuration].push(requests.len());
                    requests.push(LayoutScoreRequest {
                        slots,
                        power: p,
                        configuration,
                    });
                }
                references.push(*id);
            }
        }
        Ok(Self {
            requests,
            references: Cow::Owned(references),
            configurations,
        })
    }

    pub fn requests(&self) -> &[LayoutScoreRequest] {
        &self.requests
    }

    /// Request indices sharing one complete six-skill configuration. Prepare
    /// its power-independent bound/plan bank once, then evaluate each original
    /// f32 power separately. A group never rounds or substitutes a power.
    pub fn configurations(&self) -> &[Vec<usize>] {
        &self.configurations
    }

    /// Exact score without resolving ties between physical layouts. For a fixed
    /// skill sequence the legal hit plans are independent of power, and each
    /// plan's native note scores are nondecreasing in power (including both
    /// integer truncations). The group's largest reachable power attains its
    /// maximum. A finalist still needs `maximize` to count lower-power ties.
    pub fn maximum_monotone_score(
        &self,
        upper: &[u64],
        mut score: impl FnMut(usize, &LayoutScoreRequest) -> Result<u64, SearchError>,
    ) -> Result<u64, LayoutError> {
        if upper.len() != self.requests.len() {
            return Err(LayoutError::InvalidBounds);
        }
        let mut peaks: Vec<_> = self.configurations.iter().map(|group| {
            *group.iter().max_by_key(|&&i| self.requests[i].power)
                .expect("each configuration has a reachable layout")
        }).collect();
        peaks.sort_unstable_by_key(|&i| (std::cmp::Reverse(upper[i]), i));
        let mut best = 0;
        for i in peaks {
            if upper[i] <= best {
                break;
            }
            let value = score(i, &self.requests[i])?;
            if value > upper[i] {
                return Err(LayoutError::Underestimated { request: i, actual: value, upper: upper[i] });
            }
            best = best.max(value);
        }
        Ok(best)
    }

    /// Deterministic callback over one fixed candidate family. Equal upper
    /// bounds must still be scored so all tied native path weights are counted.
    pub fn maximize(
        &self,
        upper: &[u64],
        mut score: impl FnMut(usize, &LayoutScoreRequest) -> Result<u64, SearchError>,
    ) -> Result<LayoutMaximum, LayoutError> {
        Ok(self.maximize_optional(upper, |i, query| score(i, query).map(Some))?
            .expect("unrestricted candidates have a feasible baseline"))
    }

    /// Infeasible skill orders do not tie with a feasible zero score and do
    /// not contribute any probability mass to the recommended layout.
    pub fn maximize_optional(
        &self, upper: &[u64],
        mut score: impl FnMut(usize, &LayoutScoreRequest) -> Result<Option<u64>, SearchError>,
    ) -> Result<Option<LayoutMaximum>, LayoutError> {
        if upper.len() != self.requests.len() {
            return Err(LayoutError::InvalidBounds);
        }
        let mut pending: Vec<_> = (0..upper.len()).collect();
        pending.sort_unstable_by_key(|&i| (std::cmp::Reverse(upper[i]), i));
        let mut actual = vec![None; upper.len()];
        let mut best = 0;
        let mut scored = 0;
        for i in pending {
            if upper[i] < best {
                break;
            }
            let Some(value) = score(i, &self.requests[i])? else { scored += 1; continue; };
            if value > upper[i] {
                return Err(LayoutError::Underestimated {
                    request: i,
                    actual: value,
                    upper: upper[i],
                });
            }
            actual[i] = Some(value);
            best = best.max(value);
            scored += 1;
        }
        let mut result: Option<LayoutMaximum> = None;
        let tables = tables();
        let mut offset = 0;
        for layout in &tables.layouts {
            let ids = &self.references[offset..offset + layout.weighted_orders.len()];
            offset += ids.len();
            let mut paths = 0u16;
            let mut ties = 0;
            let mut chosen = None;
            for (&id, order) in ids.iter().zip(&layout.weighted_orders) {
                let request = usize::from(id);
                if actual[request] != Some(best) { continue; }
                let mut slots = [layout.cards[CAPTAIN_SLOT]; 6];
                slots[..5].copy_from_slice(&tables.orders[order.index]);
                paths += order.count;
                ties += 1;
                let key = (order.count, std::cmp::Reverse(request), std::cmp::Reverse(slots));
                // Iterator::max_by_key used to select the last equal key.
                if chosen.as_ref().is_none_or(|&(old, _, _)| key >= old) {
                    chosen = Some((key, request, slots));
                }
            }
            let Some((_, chosen_request, chosen_slots)) = chosen else {
                continue;
            };
            if result
                .as_ref()
                .is_some_and(|r| r.max_score_path_count >= paths)
            {
                continue;
            }
            result = Some(LayoutMaximum {
                score: best,
                recommended_layout: layout.cards,
                chosen_request,
                chosen_slots,
                max_score_path_count: paths,
                optimal_order_count: ties,
                total_path_count: SHUFFLE_PATH_COUNT as u16,
                scored_requests: scored,
                possible_requests: upper.len(),
            });
            // All native paths tie here. Later layouts cannot improve the tie
            // probability, and equal probabilities keep the first layout.
            if u64::from(paths) == SHUFFLE_PATH_COUNT {
                break;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
