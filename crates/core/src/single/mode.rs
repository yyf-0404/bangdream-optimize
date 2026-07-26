use crate::model::dp::SongMode;
use crate::model::preparation::PreparedCard;
use crate::model::schema::Attribute;

pub fn mode_candidates(cards: &[PreparedCard]) -> Vec<SongMode> {
    let mut modes = vec![SongMode::Mixed];
    let bands = card_bands(cards);
    let attributes = card_attributes(cards);

    for &band_id in &bands {
        push_useful_mode(cards, &mut modes, SongMode::UnifiedBand(band_id));
    }
    for &attribute in &attributes {
        push_useful_mode(cards, &mut modes, SongMode::UnifiedAttribute(attribute));
    }
    for &band_id in &bands {
        for &attribute in &attributes {
            push_useful_mode(
                cards,
                &mut modes,
                SongMode::UnifiedBandAttribute(band_id, attribute),
            );
        }
    }

    modes
}

fn push_useful_mode(cards: &[PreparedCard], modes: &mut Vec<SongMode>, mode: SongMode) {
    if !mode_can_build_team(cards, mode) || !mode_improves_any_skill(cards, mode) {
        return;
    }
    if modes
        .iter()
        .any(|&existing| mode_dominates(cards, existing, mode))
    {
        return;
    }

    modes.retain(|&existing| !mode_dominates(cards, mode, existing));
    modes.push(mode);
}

fn mode_can_build_team(cards: &[PreparedCard], mode: SongMode) -> bool {
    let mut characters = Vec::new();
    for card in cards.iter().filter(|card| mode.allows(card)) {
        if !characters.contains(&card.character_id) {
            characters.push(card.character_id);
            if characters.len() >= 5 {
                return true;
            }
        }
    }

    false
}

fn mode_improves_any_skill(cards: &[PreparedCard], mode: SongMode) -> bool {
    cards
        .iter()
        .filter(|card| mode.allows(card))
        .any(|card| mode_score_up(card, mode) > card.score_up.default)
}

fn mode_score_up(card: &PreparedCard, mode: SongMode) -> f64 {
    match mode {
        SongMode::Mixed => card.score_up.default,
        SongMode::UnifiedBand(band_id) => card.score_up.resolve(Some(band_id), None),
        SongMode::UnifiedAttribute(attribute) => card.score_up.resolve(None, Some(attribute)),
        SongMode::UnifiedBandAttribute(band_id, attribute) => {
            card.score_up.resolve(Some(band_id), Some(attribute))
        }
    }
}

fn mode_dominates(cards: &[PreparedCard], stronger: SongMode, weaker: SongMode) -> bool {
    cards.iter().all(|card| {
        if !weaker.allows(card) {
            return true;
        }
        if !stronger.allows(card) {
            return false;
        }
        mode_effective_score_up(card, stronger) >= mode_effective_score_up(card, weaker)
    })
}

fn mode_effective_score_up(card: &PreparedCard, mode: SongMode) -> u64 {
    if card.skill.rateup {
        return 0;
    }

    mode_score_up(card, mode).to_bits()
}

fn card_bands(cards: &[PreparedCard]) -> Vec<u32> {
    let mut bands = Vec::new();
    for card in cards {
        if !bands.contains(&card.band_id) {
            bands.push(card.band_id);
        }
    }
    bands.sort_unstable();
    bands
}

fn card_attributes(cards: &[PreparedCard]) -> Vec<Attribute> {
    const ORDER: [Attribute; 5] = [
        Attribute::Cool,
        Attribute::Happy,
        Attribute::Pure,
        Attribute::Powerful,
        Attribute::All,
    ];

    ORDER
        .into_iter()
        .filter(|attribute| cards.iter().any(|card| card.attribute == *attribute))
        .collect()
}
