//! A card-forced product envelope, complementary to the slot-assignment bound.
use super::*;

impl PoolTimingUpper {
    /// Each group contains alternatives for ONE distinct character, all from
    /// this model and the same team condition. Force each alternative in turn;
    /// choose four other characters. Power and skill maxima may come from
    /// different teammates, and the encore may come from an unselected group.
    /// These are deliberate relaxations. No prices are included in this bound.
    /// Empty groups are permitted; fewer than five nonempty groups return zero
    /// (no legal completion). Diagnostic minimum choices are not upper bounds.
    pub fn forced_product_uppers(
        &self,
        groups: &[Vec<PoolTimingChoice>],
    ) -> Result<Vec<Vec<u64>>, ReplayError> {
        self.forced_products(groups, false)
    }

    /// Same power envelope, but allocate four distinct other characters to
    /// the remaining trigger slots with exactly one captain. This retains
    /// slot and captain correlation within each chosen card. Team power and
    /// total gain are still maximized independently, so this remains an upper.
    pub fn forced_assignment_product_uppers(
        &self,
        groups: &[Vec<PoolTimingChoice>],
    ) -> Result<Vec<Vec<u64>>, ReplayError> {
        self.forced_products(groups, true)
    }

    fn forced_products(
        &self,
        groups: &[Vec<PoolTimingChoice>],
        assignment: bool,
    ) -> Result<Vec<Vec<u64>>, ReplayError> {
        if groups.iter().flatten().any(|c| c.diagnostic_minimum) {
            return Err(ReplayError::Input("forced product needs upper choices"));
        }
        let peaks: Vec<_> = groups
            .iter()
            .map(|group| {
                group.iter().fold([0_f64; 3], |v, c| {
                    [
                        v[0].max(c.linear_power),
                        v[1].max(c.gains[..5].iter().copied().fold(0., f64::max)),
                        v[2].max(c.gains[5]),
                    ]
                })
            })
            .collect();
        let mut result: Vec<_> = groups.iter().map(|g| vec![0; g.len()]).collect();
        if groups.iter().filter(|g| !g.is_empty()).count() < 5 {
            return Ok(result);
        }
        // As in linearize(): cover f32 native power accumulation and the
        // differing orders of outward f64 additions in the separable bound.
        let gamma = (linear_power_upper(1.) * (1. + 128. * f64::EPSILON)).next_up();
        let rows: Vec<[[f64; 2]; 5]> = if assignment {
            groups
                .iter()
                .map(|g| {
                    std::array::from_fn(|slot| {
                        std::array::from_fn(|captain| {
                            g.iter()
                                .map(|c| trigger_gain(c, slot, captain))
                                .fold(f64::NEG_INFINITY, f64::max)
                        })
                    })
                })
                .collect()
        } else {
            vec![]
        };
        for (g, group) in groups.iter().enumerate() {
            if group.is_empty() {
                continue;
            }
            let other: Vec<_> = peaks
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != g && !groups[*i].is_empty())
                .map(|(_, p)| *p)
                .collect();
            let top_four = |axis: usize| {
                let mut values: Vec<_> = other.iter().map(|p| p[axis]).collect();
                values.sort_by(|a, b| b.total_cmp(a));
                values[..4].iter().fold(0_f64, |v, &x| (v + x).next_up())
            };
            let other_power = top_four(0);
            let other_gain = top_four(1);
            let other_encore = other.iter().map(|p| p[2]).fold(0., f64::max);
            let mut dp = [f64::NEG_INFINITY; 64];
            dp[0] = 0.;
            if assignment {
                for (other, row) in rows.iter().enumerate() {
                    if other == g || groups[other].is_empty() {
                        continue;
                    }
                    let previous = dp;
                    for state in 0usize..64 {
                        if !previous[state].is_finite() || (state & 31).count_ones() >= 4 {
                            continue;
                        }
                        let used_captain = state >> 5;
                        for slot in 0..5 {
                            if state & (1 << slot) != 0 {
                                continue;
                            }
                            for captain in 0..=(1 - used_captain) {
                                let next = (state | (1 << slot)) + (captain << 5);
                                dp[next] =
                                    dp[next].max((previous[state] + row[slot][captain]).next_up());
                            }
                        }
                    }
                }
            }
            for (i, card) in group.iter().enumerate() {
                let power = (gamma * (card.linear_power + other_power).next_up()).next_up();
                if power > f64::from(self.max_power) {
                    return Err(ScoringError::InvalidPowerRange.into());
                }
                let gain = card.gains[..5].iter().copied().fold(0., f64::max);
                let coefficient = ((self.baseline + gain).next_up() + other_gain).next_up();
                let mut coefficient = (coefficient + other_encore.max(card.gains[5])).next_up();
                if assignment {
                    let mut gain = f64::NEG_INFINITY;
                    for slot in 0..5 {
                        for captain in 0..2 {
                            let complement = (31 ^ (1 << slot)) | ((1 - captain) << 5);
                            gain = gain.max(
                                (dp[complement] + trigger_gain(card, slot, captain)).next_up(),
                            );
                        }
                    }
                    coefficient = coefficient.min((self.baseline + gain).next_up());
                }
                let coefficient = (coefficient * (1. + 128. * f64::EPSILON)).next_up();
                result[g][i] = self.round_upper(power, coefficient);
            }
        }
        Ok(result)
    }
}

fn trigger_gain(card: &PoolTimingChoice, slot: usize, captain: usize) -> f64 {
    let normal = card.gains[slot].max(0.);
    if captain == 0 {
        normal
    } else {
        (normal + card.gains[5].max(0.)).next_up()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> PoolTimingUpper {
        PoolTimingUpper {
            adjacent: None,
            max_power: ScoringPower::new(1_000_000.).unwrap(),
            baseline: 2.35,
            error: 17.5,
            gains: vec![],
            net_stats: None,
            rounding_profile: None,
            assignment_rows: Default::default(),
            assignments: Default::default(),
        }
    }
    fn choice(power: f64, gains: [f64; 6]) -> PoolTimingChoice {
        PoolTimingChoice {
            skill: None,
            assignment_row: 0,
            diagnostic_minimum: false,
            linear_power: power,
            gains,
        }
    }

    #[test]
    fn product_encloses_every_five_character_roster_and_slot_assignment() {
        let model = model();
        // Include alternatives with anticorrelated power/skill, negative net
        // gains, an exceptional encore, empty groups and fractional powers.
        let mut groups: Vec<Vec<PoolTimingChoice>> = (0..7)
            .map(|g| {
                (0..2)
                    .map(|i| {
                        choice(
                            13000.125 + (g * 193 + i * 617) as f64,
                            std::array::from_fn(|s| {
                                ((g * 11 + s * 7 + i * 3) % 17) as f64 * 0.31 - 0.25
                            }),
                        )
                    })
                    .collect()
            })
            .collect();
        groups[3][0].gains[5] = 11.;
        groups.push(vec![]);
        let forced = model.forced_product_uppers(&groups).unwrap();
        let assigned = model.forced_assignment_product_uppers(&groups).unwrap();
        let mut audited = 0;
        for mask in 0usize..1 << 7 {
            if mask.count_ones() != 5 {
                continue;
            }
            let characters: Vec<_> = (0..7).filter(|&g| mask & (1 << g) != 0).collect();
            for alternatives in 0..32 {
                let team = std::array::from_fn(|s| groups[characters[s]][(alternatives >> s) & 1]);
                // separable_upper maximizes all 120 first-five permutations
                // and the encore; it already encloses the native score path.
                let upper = model.separable_upper(&team).unwrap();
                let power = model.separable_components(&team).unwrap().0;
                let mut allocated = 0;
                for order in crate::hit_timing::layouts::physical_layouts() {
                    for captain in 0..5 {
                        let gain = (0..5).fold(0_f64, |v, s| {
                            (v + trigger_gain(&team[order[s]], s, usize::from(s == captain)))
                                .next_up()
                        });
                        allocated = allocated
                            .max(model.round_upper(power, (model.baseline + gain).next_up()));
                    }
                }
                for s in 0..5 {
                    assert!(forced[characters[s]][(alternatives >> s) & 1] >= upper);
                    assert!(assigned[characters[s]][(alternatives >> s) & 1] >= allocated);
                    assert!(
                        assigned[characters[s]][(alternatives >> s) & 1]
                            <= forced[characters[s]][(alternatives >> s) & 1]
                    );
                }
                audited += 1;
            }
        }
        assert_eq!(audited, 672);
    }

    #[test]
    fn assignment_cannot_take_normal_and_encore_from_two_same_character_cards() {
        let model = model();
        let base = choice(1000., [1., 1., 1., 1., 1., 0.]);
        let normal = choice(1000., [10., 10., 10., 10., 10., 0.]);
        let encore = choice(1000., [0., 0., 0., 0., 0., 30.]);
        let groups = vec![
            vec![normal, encore],
            vec![base],
            vec![base],
            vec![base],
            vec![base],
        ];
        let coarse = model.forced_product_uppers(&groups).unwrap();
        let assigned = model.forced_assignment_product_uppers(&groups).unwrap();
        assert!(assigned[1][0] < coarse[1][0]);
    }

    #[test]
    fn forcing_weak_card_excludes_its_stronger_same_character_alternative() {
        let model = model();
        let weak = choice(1000., [1.; 6]);
        let strong = choice(100000., [1.; 6]);
        let groups = vec![
            vec![weak, strong],
            vec![weak],
            vec![weak],
            vec![weak],
            vec![weak],
        ];
        let forced = model.forced_product_uppers(&groups).unwrap();
        assert!(forced[0][0] < forced[0][1]);
        let linear = model.linearize(104000.).unwrap();
        let linear_upper = 5. * linear.weights(weak)[0][0] + linear.weights(weak)[0][1]
            - linear.weights(weak)[0][0]
            + linear.error();
        assert!((forced[0][0] as f64) < linear_upper);
        assert!(forced[0][0] >= model.separable_upper(&[weak; 5]).unwrap());
        assert_eq!(
            model.forced_product_uppers(&groups[..4]).unwrap(),
            vec![vec![0, 0], vec![0], vec![0], vec![0]]
        );
        assert!(model
            .forced_product_uppers(&[vec![weak.minimum_components(strong)]; 1])
            .is_err());
        assert!(model
            .forced_product_uppers(&vec![vec![choice(300000., [1.; 6])]; 5])
            .is_err());
    }
}
