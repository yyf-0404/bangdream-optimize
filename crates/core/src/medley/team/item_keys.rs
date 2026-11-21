use super::*;

// Track three cards independently. Every real plan occurs in at least one of
// the 27 allocations, including plans that leave some of these cards unused.
const KEYS: usize = 3;
const MASKS: usize = 1 << KEYS;

impl ItemScoreBounds<'_> {
    pub(crate) fn can_allocate_key_cards(
        &mut self,
        items: &SelectedAreaItems,
        minimum_score: i32,
    ) -> Result<bool, TeamBuildError> {
        let powers = adjusted_card_stats(self.cards, self.area, items);
        self.prepare_powers(&powers)?;
        // Ranking affects only pruning strength. Non-key cards remain
        // available to all three songs in every bound.
        let mut ranked: Vec<_> = self
            .profiles
            .iter()
            .enumerate()
            .map(|(i, profile)| {
                let rank: f64 = self
                    .charts
                    .iter()
                    .enumerate()
                    .map(|(s, chart)| {
                        profile.stat
                            * (chart.meta.no_skill
                                + (TEAM_SIZE + 1) as f64 * profile.best_skill_meta_by_chart[s])
                    })
                    .sum();
                (i, rank)
            })
            .collect();
        ranked.select_nth_unstable_by(KEYS - 1, |a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let keys: [usize; KEYS] = std::array::from_fn(|i| ranked[i].0);
        let mut cached = [[None; MASKS]; MEDLEY_TEAM_COUNT];
        for allocation in 0..MEDLEY_TEAM_COUNT.pow(KEYS as u32) {
            let mut assignment = allocation;
            let mut allowed = [0; MEDLEY_TEAM_COUNT];
            for bit in 0..KEYS {
                allowed[assignment % MEDLEY_TEAM_COUNT] |= 1 << bit;
                assignment /= MEDLEY_TEAM_COUNT;
            }
            let total: f64 = allowed
                .into_iter()
                .enumerate()
                .map(|(s, mask)| {
                    *cached[s][mask].get_or_insert_with(|| {
                        let excluded: Vec<_> = keys
                            .iter()
                            .enumerate()
                            .filter_map(|(bit, &i)| (mask & (1 << bit) == 0).then_some(i))
                            .collect();
                        chart_upper_excluding(
                            self.cards,
                            &self.profiles,
                            &self.charts[s],
                            s,
                            &self.signatures,
                            &excluded,
                        )
                    })
                })
                .sum();
            if total >= f64::from(minimum_score) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
