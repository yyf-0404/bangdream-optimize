use crate::model::preparation::PreparedCard;
use crate::model::schema::Attribute;

const TEAM_SIZE: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::medley) enum MedleyPruneSignature {
    Mixed,
    UnifiedBand(u32),
    UnifiedAttribute(Attribute),
    UnifiedBandAttribute(u32, Attribute),
}

impl MedleyPruneSignature {
    pub(in crate::medley) fn allows(self, card: &PreparedCard) -> bool {
        match self {
            Self::Mixed => true,
            Self::UnifiedBand(band_id) => card.band_id == band_id,
            Self::UnifiedAttribute(attribute) => card.attribute == attribute,
            Self::UnifiedBandAttribute(band_id, attribute) => {
                card.band_id == band_id && card.attribute == attribute
            }
        }
    }

    pub(in crate::medley) fn team_band_id(self) -> Option<u32> {
        match self {
            Self::Mixed | Self::UnifiedAttribute(_) => None,
            Self::UnifiedBand(band_id) | Self::UnifiedBandAttribute(band_id, _) => Some(band_id),
        }
    }

    pub(in crate::medley) fn team_attribute(self) -> Option<Attribute> {
        match self {
            Self::Mixed | Self::UnifiedBand(_) => None,
            Self::UnifiedAttribute(attribute) | Self::UnifiedBandAttribute(_, attribute) => {
                Some(attribute)
            }
        }
    }
}

pub(in crate::medley) fn seed_signatures(cards: &[PreparedCard]) -> Vec<MedleyPruneSignature> {
    let mut signatures = vec![MedleyPruneSignature::Mixed];
    for card in cards {
        push_seed_signature(
            &mut signatures,
            MedleyPruneSignature::UnifiedBand(card.band_id),
        );
        push_seed_signature(
            &mut signatures,
            MedleyPruneSignature::UnifiedAttribute(card.attribute),
        );
        push_seed_signature(
            &mut signatures,
            MedleyPruneSignature::UnifiedBandAttribute(card.band_id, card.attribute),
        );
    }
    signatures
}

fn push_seed_signature(
    signatures: &mut Vec<MedleyPruneSignature>,
    signature: MedleyPruneSignature,
) {
    if !signatures.contains(&signature) {
        signatures.push(signature);
    }
}

pub(in crate::medley) fn signature_label(signature: MedleyPruneSignature) -> String {
    match signature {
        MedleyPruneSignature::Mixed => "mixed".to_owned(),
        MedleyPruneSignature::UnifiedBand(band_id) => format!("unifiedBand({band_id})"),
        MedleyPruneSignature::UnifiedAttribute(attribute) => {
            format!("unifiedAttribute({})", attribute_label(attribute))
        }
        MedleyPruneSignature::UnifiedBandAttribute(band_id, attribute) => {
            format!(
                "unifiedBandAttribute({band_id},{})",
                attribute_label(attribute)
            )
        }
    }
}

fn attribute_label(attribute: Attribute) -> &'static str {
    match attribute {
        Attribute::Cool => "cool",
        Attribute::Happy => "happy",
        Attribute::Pure => "pure",
        Attribute::Powerful => "powerful",
        Attribute::All => "all",
    }
}

pub(in crate::medley) fn signature_improves_any_skill(
    cards: &[PreparedCard],
    signature: MedleyPruneSignature,
) -> bool {
    cards
        .iter()
        .filter(|card| signature.allows(card))
        .any(|card| {
            card.score_up
                .resolve(signature.team_band_id(), signature.team_attribute())
                > card.score_up.default
        })
}

pub(in crate::medley) fn signature_can_complete_with_card(
    cards: &[PreparedCard],
    idx: usize,
    signature: MedleyPruneSignature,
) -> bool {
    let Some(card) = cards.get(idx) else {
        return false;
    };
    if !signature.allows(card) {
        return false;
    }

    let mut characters = vec![card.character_id];
    for (other_idx, other) in cards.iter().enumerate() {
        if other_idx == idx || !signature.allows(other) || characters.contains(&other.character_id)
        {
            continue;
        }
        characters.push(other.character_id);
        if characters.len() >= TEAM_SIZE {
            return true;
        }
    }

    false
}
