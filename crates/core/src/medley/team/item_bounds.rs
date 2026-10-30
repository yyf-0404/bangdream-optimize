//! Chart/skill data is fixed during an item search; only adjusted stats change.
use super::*;
use crate::team_prune::{
    priced::{CoarseSignature, ItemAnalysis},
    MedleyCardPruneProfile,
};

pub(crate) struct ItemBoundContext<'a> {
    cards: &'a [PreparedCard],
    charts: &'a [Chart],
    area: &'a AreaItemPercent,
    profiles: Vec<MedleyCardPruneProfile>,
    signatures: Vec<MedleyPruneSignature>,
    coarse_by_song: Vec<Vec<Option<CoarseSignature>>>,
}

impl<'a> ItemBoundContext<'a> {
    pub(crate) fn new(
        cards: &'a [PreparedCard],
        charts: &'a [Chart],
        area: &'a AreaItemPercent,
    ) -> Result<Self, TeamBuildError> {
        let profiles = medley_card_prune_profiles(cards, charts, &vec![0.0; cards.len()])?;
        let signatures = seed_signatures(cards);
        let coarse_by_song = charts
            .iter()
            .enumerate()
            .map(|(song, chart)| {
                signatures
                    .iter()
                    .map(|&signature| {
                        CoarseSignature::new(cards, &profiles, chart, song, signature)
                    })
                    .collect()
            })
            .collect();
        Ok(Self {
            cards,
            charts,
            area,
            profiles,
            signatures,
            coarse_by_song,
        })
    }

    fn set_items(&mut self, items: &SelectedAreaItems) {
        for (profile, card) in self.profiles.iter_mut().zip(self.cards) {
            profile.stat = card.add_up_stat(
                self.area,
                &items.band,
                &items.attribute,
                items.magazine.as_str(),
            );
        }
    }

    pub(crate) fn coarse(&mut self, items: &SelectedAreaItems) -> i32 {
        self.set_items(items);
        self.charts
            .iter()
            .enumerate()
            .map(|(song, chart)| {
                self.coarse_by_song[song]
                    .iter_mut()
                    .map(|prepared| {
                        prepared.as_mut().map_or(f64::INFINITY, |prepared| {
                            prepared.bound(&self.profiles, chart)
                        })
                    })
                    .fold(0.0, f64::max) as i32
            })
            .fold(0i32, i32::saturating_add)
    }

    pub(crate) fn analyze(&mut self, items: &SelectedAreaItems, floor: i32) -> ItemAnalysis {
        self.set_items(items);
        priced_item_analysis_from_profiles(
            self.cards,
            self.charts,
            &self.profiles,
            &self.signatures,
            floor,
        )
    }
}
