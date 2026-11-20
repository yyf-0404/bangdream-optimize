//! Exact request sharing across all physical layouts, captains and native orders.
//! Full resolved skill sequences and original f32 power bits are coalesced;
//! every physical layout retains its actual card order and native path weights.
use super::{search::SearchError, ReplayError, ReplaySkill};
use crate::{
    scoring::{ScoringError, ScoringPower},
    skill_shuffle::{tables, CAPTAIN_SLOT, SHUFFLE_PATH_COUNT},
};
use std::collections::BTreeMap;

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
    references: Box<[u16]>,
    configurations: Vec<Vec<usize>>,
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
        let mut keys = BTreeMap::new();
        let mut configuration_keys = BTreeMap::new();
        let mut configurations: Vec<Vec<usize>> = Vec::new();
        let mut requests = Vec::new();
        let mut references = Vec::with_capacity(
            tables.layouts.iter().map(|l| l.weighted_orders.len()).sum(),
        );
        for layout in &tables.layouts {
            let p = power(layout.cards)?;
            let captain = layout.cards[CAPTAIN_SLOT];
            for order in &layout.weighted_orders {
                let mut slots = [captain; 6];
                slots[..5].copy_from_slice(&tables.orders[order.index]);
                let sequence = slots.map(|i| classes[i]);
                let configuration = *configuration_keys.entry(sequence).or_insert_with(|| {
                    configurations.push(Vec::new());
                    configurations.len() - 1
                });
                let key = (sequence, p.cache_key());
                let index = *keys.entry(key).or_insert_with(|| {
                    configurations[configuration].push(requests.len());
                    requests.push(LayoutScoreRequest {
                        slots,
                        power: p,
                        configuration,
                    });
                    requests.len() - 1
                });
                references.push(u16::try_from(index)
                    .expect("five-card layout family has at most 120 * 96 requests"));
            }
        }
        Ok(Self {
            requests,
            references: references.into_boxed_slice(),
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
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
