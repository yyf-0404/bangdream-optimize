//! Keep physical member order separate from skill activation order.
use super::{DeckPower, MemberPower, ScoringPower};
use std::{borrow::Cow, sync::OnceLock};

#[derive(Debug, Clone, Copy)]
pub enum TeamPower {
    /// Compatibility for callers which already resolved a scalar power.
    Fixed(ScoringPower),
    Members([MemberPower; 5]),
}

#[derive(Debug, Clone)]
pub struct TeamPowerGroup {
    pub power: ScoringPower,
    pub layouts: Cow<'static, [usize]>,
    pub reachable: [u128; 5],
}

/// Most teams have one proven first-stage score plateau. Keep that group on
/// the stack; only genuinely distinct powers need a heap-backed collection.
#[derive(Debug, Clone)]
pub(crate) enum TeamPowerGroups {
    Single(Option<TeamPowerGroup>),
    Multiple(Vec<TeamPowerGroup>),
}

impl std::ops::Deref for TeamPowerGroups {
    type Target = [TeamPowerGroup];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Single(group) => group.as_slice(),
            Self::Multiple(groups) => groups,
        }
    }
}

impl TeamPowerGroups {
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&TeamPowerGroup) -> bool) {
        match self {
            Self::Single(group) => {
                if group.as_ref().is_some_and(|value| !keep(value)) {
                    *group = None;
                }
            }
            Self::Multiple(groups) => groups.retain(keep),
        }
    }
}

impl<'a> IntoIterator for &'a mut TeamPowerGroups {
    type Item = &'a mut TeamPowerGroup;
    type IntoIter = std::slice::IterMut<'a, TeamPowerGroup>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            TeamPowerGroups::Single(group) => group.as_mut_slice().iter_mut(),
            TeamPowerGroups::Multiple(groups) => groups.iter_mut(),
        }
    }
}

impl IntoIterator for TeamPowerGroups {
    type Item = TeamPowerGroup;
    type IntoIter =
        std::iter::Chain<std::option::IntoIter<TeamPowerGroup>, std::vec::IntoIter<TeamPowerGroup>>;

    fn into_iter(self) -> Self::IntoIter {
        let (single, multiple) = match self {
            Self::Single(group) => (group, Vec::new()),
            Self::Multiple(groups) => (None, groups),
        };
        single.into_iter().chain(multiple)
    }
}

impl TeamPower {
    pub fn power(self, layout: [usize; 5]) -> ScoringPower {
        match self {
            Self::Fixed(value) => value,
            Self::Members(members) => {
                ScoringPower::from_member_parameters(&layout.map(|i| members[i].final_parameters()))
                    .expect("validated member parameters")
            }
        }
    }

    pub fn display_unrounded(self, layout: [usize; 5]) -> f32 {
        match self {
            Self::Fixed(value) => value.value(),
            Self::Members(members) => DeckPower::from_members(&layout.map(|i| members[i]))
                .expect("validated member parameters")
                .display_unrounded(),
        }
    }

    pub fn score_attack_display_integer(self, layout: [usize; 5]) -> i32 {
        match self {
            Self::Fixed(value) => value.value() as i32,
            Self::Members(members) => DeckPower::from_members(&layout.map(|i| members[i]))
                .expect("validated member parameters")
                .score_attack_display_integer()
                .expect("validated display power"),
        }
    }

    /// Merge physical powers only when every chart proves that their first
    /// integer note scores are identical. Later Fever/skill/queue operations
    /// then receive identical inputs for every possible skill timeline.
    pub(crate) fn groups_for_charts(
        self,
        charts: &[crate::Chart],
        medley: bool,
    ) -> TeamPowerGroups {
        let sum = match self {
            Self::Fixed(value) => value.value() as f64,
            Self::Members(members) => members.into_iter().map(MemberPower::linear_total).sum(),
        };
        let low = super::linear_power_lower(sum);
        let high = super::linear_power_upper(sum);
        let plateau = charts
            .iter()
            .all(|chart| chart.native_base_scores_equal(low, high, medley));
        super::cache_audit::lookup(super::cache_audit::Cache::PowerPlateau, plateau, 0);
        if plateau {
            static ALL: OnceLock<([usize; 120], [u128; 5])> = OnceLock::new();
            let (layouts, reachable) = ALL.get_or_init(|| {
                let mut reachable = [0; 5];
                for layout in &crate::skill_shuffle::tables().layouts {
                    reachable[layout.cards[2]] |= layout.reachable;
                }
                (std::array::from_fn(|i| i), reachable)
            });
            return TeamPowerGroups::Single(Some(TeamPowerGroup {
                power: self.power([0, 1, 2, 3, 4]),
                layouts: Cow::Borrowed(layouts),
                reachable: *reachable,
            }));
        }
        TeamPowerGroups::Multiple(self.groups())
    }

    /// Each distinct float power is scored once, then shared only with the
    /// physical arrangements having that exact bit pattern.
    pub fn groups(self) -> Vec<TeamPowerGroup> {
        let mut groups: Vec<TeamPowerGroup> = Vec::with_capacity(3);
        let table = crate::skill_shuffle::tables();
        let axes = match self {
            Self::Members(members) => members.map(MemberPower::final_parameters),
            Self::Fixed(_) => [[0.0; 3]; 5],
        };
        // Shared prefixes retain the native left-to-right accumulation. Never
        // reassociate the final two members into a pair sum.
        let mut triples = [[[[0.0_f32; 3]; 5]; 5]; 5];
        for a in 0..5 {
            for b in 0..5 {
                if a == b {
                    continue;
                }
                let pair: [f32; 3] = std::array::from_fn(|k| axes[a][k] + axes[b][k]);
                for c in 0..5 {
                    if a == c || b == c {
                        continue;
                    }
                    triples[a][b][c] = std::array::from_fn(|k| pair[k] + axes[c][k]);
                }
            }
        }
        for (index, layout) in table.layouts.iter().enumerate() {
            let [a, b, c, d, e] = layout.cards;
            let totals: [f32; 3] =
                std::array::from_fn(|k| (triples[a][b][c][k] + axes[d][k]) + axes[e][k]);
            let power = match self {
                Self::Fixed(value) => value,
                Self::Members(_) => ScoringPower::new(totals[1] + (totals[0] + totals[2]))
                    .expect("validated members"),
            };
            if let Some(group) = groups.iter_mut().find(|group| group.power == power) {
                group.layouts.to_mut().push(index);
                group.reachable[layout.cards[2]] |= layout.reachable;
            } else {
                let mut reachable = [0; 5];
                reachable[layout.cards[2]] = layout.reachable;
                let mut layouts = Vec::with_capacity(120);
                layouts.push(index);
                groups.push(TeamPowerGroup {
                    power,
                    layouts: Cow::Owned(layouts),
                    reachable,
                });
            }
        }
        groups.sort_unstable_by_key(|group| std::cmp::Reverse(group.power));
        groups
    }
}

impl From<ScoringPower> for TeamPower {
    fn from(value: ScoringPower) -> Self {
        Self::Fixed(value)
    }
}
impl From<i32> for TeamPower {
    fn from(value: i32) -> Self {
        Self::Fixed(ScoringPower::new(value as f32).expect("nonnegative power"))
    }
}
impl From<f64> for TeamPower {
    fn from(value: f64) -> Self {
        Self::Fixed(ScoringPower::new(value as f32).expect("finite nonnegative power"))
    }
}
impl From<[MemberPower; 5]> for TeamPower {
    fn from(value: [MemberPower; 5]) -> Self {
        Self::Members(value)
    }
}
