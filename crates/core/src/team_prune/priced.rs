//! A coupled power/skill bound with distinct characters and activation slots.
//! For P,M >= 0 and lambda > 0, P*M <= (lambda*P + M/lambda)^2/4.
//! A 5-slot assignment DP maximizes the linear expression. The repeat is attached
//! to exactly one selected card, so its power and its two activations stay coupled.
use super::{MedleyCardPruneProfile, MedleyPruneSignature};
use crate::model::{
    chart::{Chart, TeamCardSkill},
    preparation::PreparedCard,
};
use std::collections::BTreeMap;

mod item;
pub(crate) use item::conditional_item_bound;
pub(crate) use item::{conditional_item_analysis, ItemAnalysis, ResourceBound};

pub(crate) fn enabled() -> bool {
    #[cfg(all(feature = "experimental-priced-prune", not(target_arch = "wasm32")))]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        return *ENABLED.get_or_init(|| {
            std::env::var("BANGDREAM_OPTIMIZE_PRICED_PRUNE").as_deref() != Ok("off")
        });
    }
    #[allow(unreachable_code)]
    true
}

pub(crate) fn item_enabled() -> bool {
    #[cfg(all(feature = "experimental-priced-prune", not(target_arch = "wasm32")))]
    if std::env::var("BANGDREAM_OPTIMIZE_PRICED_ITEM_PRUNE").as_deref() == Ok("off") {
        return false;
    }
    enabled()
}

#[derive(Clone, Copy, Debug)]
struct Choice {
    power: f64,
    skill: [f64; 6],
}

fn signature_choices(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    chart: &Chart,
    chart_idx: usize,
    signature: MedleyPruneSignature,
) -> Option<Vec<Vec<Choice>>> {
    let mut groups: BTreeMap<u32, Vec<Choice>> = BTreeMap::new();
    let mut rateup_windows = BTreeMap::<(u64, u64), [f64; 6]>::new();
    for (card, profile) in cards.iter().zip(profiles) {
        if !signature.allows(card) {
            continue;
        }
        let up = card
            .score_up
            .resolve(signature.team_band_id(), signature.team_attribute());
        let Some(meta) = profile.skill_meta_for_score_up(up) else {
            return None;
        };
        let Some(values) = meta.get(chart_idx * 6..chart_idx * 6 + 6) else {
            return None;
        };
        let mut values: [f64; 6] = values.try_into().unwrap();
        if card.skill.rateup && chart.warning.is_empty() {
            // The legacy no-queue rate-up summary assumes score_up=1.0. A tight
            // bound must use the real multiplier recurrence, including variants.
            let key = (card.skill.duration.to_bits(), up.to_bits());
            if let Some(cached) = rateup_windows.get(&key) {
                values = *cached;
            } else {
                let skill = TeamCardSkill {
                    score_up: up,
                    ..card.skill
                };
                let Ok(conditional) = chart.conditional_skill_meta(skill) else {
                    return None;
                };
                values = conditional.map(|row| row.into_iter().fold(0.0, f64::max));
                rateup_windows.insert(key, values);
            }
        }
        if !profile.stat.is_finite()
            || profile.stat < 0.0
            || values.iter().any(|x| !x.is_finite() || *x < 0.0)
        {
            return None;
        }
        groups.entry(card.character_id).or_default().push(Choice {
            power: profile.stat,
            skill: values,
        });
    }
    Some(groups.into_values().collect())
}

struct CoarseCard {
    index: usize,
    power: f64,
    normal: f64,
    captain: f64,
}

pub(crate) struct CoarseSignature {
    groups: Vec<Vec<CoarseCard>>,
}

impl CoarseSignature {
    pub(crate) fn new(
        cards: &[PreparedCard],
        profiles: &[MedleyCardPruneProfile],
        chart: &Chart,
        chart_idx: usize,
        signature: MedleyPruneSignature,
    ) -> Option<Self> {
        let choices = signature_choices(cards, profiles, chart, chart_idx, signature)?;
        let mut indices = BTreeMap::<u32, Vec<usize>>::new();
        for (index, card) in cards.iter().enumerate() {
            if signature.allows(card) {
                indices.entry(card.character_id).or_default().push(index);
            }
        }
        let groups = choices
            .into_iter()
            .zip(indices.into_values())
            .map(|(group, indices)| {
                group
                    .into_iter()
                    .zip(indices)
                    .map(|(choice, index)| CoarseCard {
                        index,
                        power: choice.power,
                        normal: choice.skill[..5].iter().copied().fold(0.0, f64::max),
                        captain: choice.skill[5],
                    })
                    .collect()
            })
            .collect();
        Some(Self { groups })
    }

    pub(crate) fn bound(&mut self, profiles: &[MedleyCardPruneProfile], chart: &Chart) -> f64 {
        for card in self.groups.iter_mut().flatten() {
            let power = profiles[card.index].stat;
            if !power.is_finite() || power < 0.0 {
                return f64::INFINITY;
            }
            card.power = power;
        }
        let groups = &self.groups;
        if groups.len() < 5 {
            return 0.0;
        }
        let power = top_five(
            groups
                .iter()
                .map(|g| g.iter().map(|c| c.power).fold(0.0, f64::max)),
        );
        let captain = groups
            .iter()
            .flatten()
            .map(|c| c.captain)
            .fold(0.0, f64::max);
        let skill = top_five(
            groups
                .iter()
                .map(|g| g.iter().map(|c| c.normal).fold(0.0, f64::max)),
        ) + captain
            + chart.meta.no_skill;
        let balanced = (skill / power).sqrt();
        if !balanced.is_finite() || balanced <= 0.0 {
            return f64::INFINITY;
        }
        [0.8, 1.0, 1.25]
            .into_iter()
            .map(|scale| {
                let lambda = balanced * scale;
                let normal = top_five(groups.iter().map(|g| {
                    g.iter()
                        .map(|c| {
                            ((lambda * c.power).next_up() + (c.normal / lambda).next_up()).next_up()
                        })
                        .fold(0.0, f64::max)
                }));
                let linear = (normal.next_up()
                    + ((captain + chart.meta.no_skill).next_up() / lambda).next_up())
                .next_up();
                let upper = ((linear * linear).next_up() / 4.0).next_up();
                (upper + upper.abs() * f64::EPSILON * 32.0 * (chart.count as f64 + 64.0))
                    .next_up()
                    .ceil()
            })
            .fold(f64::INFINITY, f64::min)
    }
}

pub(crate) fn signature_bound(
    cards: &[PreparedCard],
    profiles: &[MedleyCardPruneProfile],
    chart: &Chart,
    chart_idx: usize,
    signature: MedleyPruneSignature,
) -> f64 {
    let Some(groups) = signature_choices(cards, profiles, chart, chart_idx, signature) else {
        return f64::INFINITY;
    };
    if groups.len() < 5 {
        return 0.0;
    }
    let power = top_five(
        groups
            .iter()
            .map(|g| g.iter().map(|c| c.power).fold(0.0, f64::max)),
    );
    let skill = top_five(groups.iter().map(|g| {
        g.iter()
            .map(|c| c.skill[..5].iter().copied().fold(0.0, f64::max))
            .fold(0.0, f64::max)
    })) + groups
        .iter()
        .flatten()
        .map(|c| c.skill[5])
        .fold(0.0, f64::max)
        + chart.meta.no_skill;
    if !power.is_finite() || !skill.is_finite() || power <= 0.0 || skill <= 0.0 {
        return f64::INFINITY;
    }
    let balanced = (skill / power).sqrt();
    [0.8, 1.0, 1.25]
        .into_iter()
        .map(|scale| {
            let lambda = balanced * scale;
            let linear = (assignment_bound(&groups, lambda)
                + (chart.meta.no_skill / lambda).next_up())
            .next_up();
            let score = ((linear * linear).next_up() / 4.0).next_up();
            // The metadata itself sums positive per-judgment floating-point terms.
            // Inflate for that accumulation, plus the DP additions/reassociation.
            // This only weakens pruning; the exact integer scorer is unchanged.
            let error = score.abs() * f64::EPSILON * 32.0 * (chart.count as f64 + 64.0);
            (score + error).next_up().ceil()
        })
        .fold(f64::INFINITY, f64::min)
}

fn top_five(values: impl Iterator<Item = f64>) -> f64 {
    let mut best = [0.0; 5];
    for value in values {
        if value > best[4] {
            best[4] = value;
            best.sort_by(|a, b| b.total_cmp(a));
        }
    }
    best.into_iter().sum()
}

fn assignment_bound(groups: &[Vec<Choice>], lambda: f64) -> f64 {
    // bit 5 records whether the captain's repeat has been assigned.
    let mut dp = [f64::NEG_INFINITY; 64];
    dp[0] = 0.0;
    for group in groups {
        let mut normal = [f64::NEG_INFINITY; 5];
        let mut captain = [f64::NEG_INFINITY; 5];
        for card in group {
            let p = (lambda * card.power).next_up();
            for slot in 0..5 {
                normal[slot] =
                    normal[slot].max((p + (card.skill[slot] / lambda).next_up()).next_up());
                captain[slot] = captain[slot].max(
                    (p + ((card.skill[slot] + card.skill[5]).next_up() / lambda).next_up())
                        .next_up(),
                );
            }
        }
        // Descending masks prevent selecting the current character twice.
        for mask in (0usize..31).rev() {
            for slot in 0..5 {
                let bit = 1 << slot;
                if mask & bit != 0 {
                    continue;
                }
                let next = mask | bit;
                if dp[mask].is_finite() {
                    dp[next] = dp[next].max((dp[mask] + normal[slot]).next_up());
                    dp[next | 32] = dp[next | 32].max((dp[mask] + captain[slot]).next_up());
                }
                if dp[mask | 32].is_finite() {
                    dp[next | 32] = dp[next | 32].max((dp[mask | 32] + normal[slot]).next_up());
                }
            }
        }
    }
    dp[63]
}
