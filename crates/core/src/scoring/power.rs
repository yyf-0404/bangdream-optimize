use super::{nonnegative, ScoringError};
use crate::{AreaItemPercent, LiveVariant, PreparedCard, SelectedAreaItems};

/// Aggregate targeted item values in equipment order before dividing by 100.
/// Inputs are master-data percentages, not already-divided per-group rates.
#[derive(Debug, Clone, Copy)]
pub struct AreaBonus {
    pub fixed: [f32; 3],
    pub rate: [f32; 3],
}

impl AreaBonus {
    pub fn from_effects(
        fixed_values: &[[f32; 3]],
        percentages: &[[f32; 3]],
    ) -> Result<Self, ScoringError> {
        let mut fixed = [0.0_f32; 3];
        let mut rate = [0.0_f32; 3];
        for values in fixed_values {
            for i in 0..3 {
                fixed[i] += nonnegative(values[i], "area fixed value")?;
            }
        }
        for values in percentages {
            for i in 0..3 {
                rate[i] += nonnegative(values[i], "area percentage")?;
            }
        }
        for i in 0..3 {
            fixed[i] = nonnegative(fixed[i], "area fixed total")?;
            rate[i] = nonnegative(rate[i] / 100.0_f32, "area rate total")?;
        }
        Ok(Self { fixed, rate })
    }
}

/// Nonnegative finite f32, retaining exact fractional bits for scoring caches.
/// +0 and -0 are canonicalized; nonnegative IEEE bits also preserve ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScoringPower(u32);

impl ScoringPower {
    pub fn new(value: f32) -> Result<Self, ScoringError> {
        Ok(Self(nonnegative(value, "scoring power")?.to_bits()))
    }
    pub fn value(self) -> f32 {
        f32::from_bits(self.0)
    }
    pub fn cache_key(self) -> u32 {
        self.0
    }

    /// Member order matters. Rows are [performance, technique, visual].
    pub fn from_member_parameters(members: &[[f32; 3]]) -> Result<Self, ScoringError> {
        Self::from_parameters(members.iter().copied())
    }

    /// Maximum scoring power over all physical layouts of these five members.
    pub fn maximum_layout_power(members: &[[f32; 3]; 5]) -> Result<Self, ScoringError> {
        for &value in members.iter().flatten() {
            nonnegative(value, "member parameter")?;
        }
        // Only the first two members commute. Share their sum and try every
        // ordering of the remaining three with native left-to-right rounding.
        const PAIRS: [([usize; 2], [usize; 3]); 10] = [
            ([0, 1], [2, 3, 4]),
            ([0, 2], [1, 3, 4]),
            ([0, 3], [1, 2, 4]),
            ([0, 4], [1, 2, 3]),
            ([1, 2], [0, 3, 4]),
            ([1, 3], [0, 2, 4]),
            ([1, 4], [0, 2, 3]),
            ([2, 3], [0, 1, 4]),
            ([2, 4], [0, 1, 3]),
            ([3, 4], [0, 1, 2]),
        ];
        let mut maximum = 0.0_f32;
        for ([a, b], rest) in PAIRS {
            let pair: [f32; 3] = std::array::from_fn(|i| members[a][i] + members[b][i]);
            let [x, y, z] = rest.map(|i| members[i]);
            for (third, fourth, fifth) in [(x, y, z), (y, z, x), (z, x, y)] {
                let prefix: [f32; 3] = std::array::from_fn(|i| pair[i] + third[i]);
                for (d, e) in [(fourth, fifth), (fifth, fourth)] {
                    let totals: [f32; 3] = std::array::from_fn(|i| (prefix[i] + d[i]) + e[i]);
                    maximum = maximum.max(totals[1] + (totals[0] + totals[2]));
                }
            }
        }
        Self::new(maximum)
    }

    fn from_parameters(members: impl Iterator<Item = [f32; 3]>) -> Result<Self, ScoringError> {
        let mut totals = [0.0_f32; 3];
        for member in members {
            for i in 0..3 {
                totals[i] += nonnegative(member[i], "member parameter")?;
            }
        }
        // ScoreUtility 0x21c9a64: (performance + visual), then technique.
        Self::new(totals[1] + (totals[0] + totals[2]))
    }
}

impl From<ScoringPower> for f64 {
    fn from(value: ScoringPower) -> Self {
        value.value() as f64
    }
}

/// Integer member parameters plus already-resolved item/event inputs.
/// Selection, rate aggregation and growth are separate caller responsibilities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemberPower {
    original: [f32; 3],
    with_area: [f32; 3],
    common_event: [f32; 3],
    extra_event: [f32; 3],
    final_parameters: [f32; 3],
}

impl MemberPower {
    /// Compatibility input for explicitly constructed, pre-migration core
    /// fixtures. Application preparation always supplies original percentages.
    pub(crate) fn from_resolved_parts(original: [f32; 3], rate: [f32; 3], event: [f32; 3]) -> Self {
        let with_area = std::array::from_fn(|i| original[i] * (1.0 + rate[i]));
        Self {
            original,
            with_area,
            common_event: event,
            extra_event: [0.0; 3],
            final_parameters: std::array::from_fn(|i| with_area[i] + event[i]),
        }
    }

    /// Exact real sum of the already-rounded axes, used only for search bounds.
    pub fn linear_total(self) -> f64 {
        self.final_parameters.into_iter().map(f64::from).sum()
    }

    /// Includes display-path operands; equal scoring axes alone do not identify
    /// cooperative sixth-player selection at an integer display boundary.
    pub(crate) fn cache_key(self) -> [u32; 15] {
        let mut values = [0; 15];
        for (chunk, axes) in values.chunks_exact_mut(3).zip([
            self.original,
            self.with_area,
            self.common_event,
            self.extra_event,
            self.final_parameters,
        ]) {
            chunk.copy_from_slice(&axes.map(f32::to_bits));
        }
        values
    }

    pub fn componentwise_dominates(self, other: Self) -> bool {
        (0..3).all(|i| self.final_parameters[i] >= other.final_parameters[i])
    }
    pub fn from_bonuses(
        original: [u32; 3],
        area_fixed: [f32; 3],
        area_rate: [f32; 3],
        event_percent: f32,
        extra_event: [f32; 3],
    ) -> Result<Self, ScoringError> {
        let event_percent = nonnegative(event_percent, "event percent")?;
        let event_multiplier = (event_percent + 100.0_f32) / 100.0_f32;
        let original = original.map(|value| value as f32);
        let mut with_area = [0.0_f32; 3];
        let mut common_event = [0.0_f32; 3];
        let mut final_parameters = [0.0_f32; 3];
        for i in 0..3 {
            let fixed = nonnegative(area_fixed[i], "area fixed bonus")?;
            let rate = nonnegative(area_rate[i], "area rate")?;
            let extra = nonnegative(extra_event[i], "extra event parameter")?;
            with_area[i] = (original[i] + fixed) * (1.0_f32 + rate);
            // ARM32 vnmls: rounded multiplication, then subtract original.
            // Do not replace with original * (event_percent / 100), or mul_add.
            common_event[i] = (original[i] * event_multiplier) - original[i];
            let with_event = with_area[i] + common_event[i];
            final_parameters[i] = nonnegative(with_event + extra, "final member parameter")?;
        }
        Ok(Self {
            original,
            with_area,
            common_event,
            extra_event,
            final_parameters,
        })
    }
    pub fn final_parameters(self) -> [f32; 3] {
        self.final_parameters
    }
}

/// Separate scoring and display paths, with fractional intermediate values.
#[derive(Debug, Clone, Copy)]
pub struct DeckPower {
    scoring: ScoringPower,
    display_unrounded: f32,
    original_total: f32,
    area_total: f32,
    with_area_total: f32,
}

fn sum_axes(values: [f32; 3]) -> f32 {
    (values[0] + values[1]) + values[2]
}

fn original_and_area_totals(members: &[MemberPower]) -> (f32, f32) {
    let mut original = [0.0_f32; 3];
    let mut area = [0.0_f32; 3];
    for member in members {
        for i in 0..3 {
            original[i] += member.original[i];
            area[i] += member.with_area[i] - member.original[i];
        }
    }
    (sum_axes(original), sum_axes(area))
}

pub(super) fn cooperative_room_stat(members: &[MemberPower]) -> i32 {
    // ScreenLayerMultiPlayLiveLevelSelect.get_DeckTotalParam passes this
    // integer to room selection before the song starts.
    let (original, area) = original_and_area_totals(members);
    (original + area) as i32
}

impl DeckPower {
    pub fn from_members(members: &[MemberPower]) -> Result<Self, ScoringError> {
        let (original_total, area_total) = original_and_area_totals(members);
        let mut event = [0.0_f32; 3];
        for member in members {
            for i in 0..3 {
                event[i] += member.common_event[i];
                // Pre-adding this to common_event changes the rounding order.
                event[i] += member.extra_event[i];
            }
        }
        let with_area_total = original_total + area_total;
        let display_unrounded = with_area_total + sum_axes(event);
        Ok(Self {
            scoring: ScoringPower::from_parameters(members.iter().map(|m| m.final_parameters))?,
            display_unrounded: nonnegative(display_unrounded, "display power")?,
            original_total: nonnegative(original_total, "original power")?,
            area_total: nonnegative(area_total, "area power")?,
            with_area_total: nonnegative(with_area_total, "power with area")?,
        })
    }

    pub(crate) fn from_selected_cards(
        card_ids: [u32; 5],
        cards: &[PreparedCard],
        area: &AreaItemPercent,
        items: &SelectedAreaItems,
    ) -> Result<Self, ScoringError> {
        let members = card_ids.map(|id| {
            let card = cards
                .iter()
                .find(|card| card.card_id == id)
                .expect("selected card belongs to the input pool");
            card.member_power(area, &items.band, &items.attribute, items.magazine.as_str())
        });
        Self::from_members(&members)
    }
    pub fn scoring(self) -> ScoringPower {
        self.scoring
    }
    pub fn display_unrounded(self) -> f32 {
        self.display_unrounded
    }
    pub fn display_integer(self) -> Result<i32, ScoringError> {
        display_integer(self.display_unrounded)
    }

    pub fn display_for_live(self, live_variant: LiveVariant) -> Result<i32, ScoringError> {
        match live_variant {
            // DeckSelectViewer and ScreenLayerMultiPlayLiveLevelSelect read
            // DeckParamDetail.TotalParameterIncludeAreaItem.
            LiveVariant::Solo | LiveVariant::Cooperative => display_integer(self.with_area_total),
            LiveVariant::ChallengeCp => self.score_attack_display_integer(),
            // Versus and Festival rooms and per-deck Medley use the inclusive total.
            LiveVariant::Versus | LiveVariant::Festival | LiveVariant::Medley => {
                self.display_integer()
            }
        }
    }

    /// The score-attack screen truncates the deck's pre-event total, then adds
    /// its displayed event bonus. The bonus is `int(total - area) - int(original)`
    /// in the CN 9.4.4 UI, so truncating the full float gives a different value
    /// for some teams.
    pub fn score_attack_display_integer(self) -> Result<i32, ScoringError> {
        let with_area = display_integer(self.with_area_total)?;
        let without_area = display_integer(self.display_unrounded - self.area_total)?;
        if without_area == 0 {
            return Ok(with_area);
        }
        let original = display_integer(self.original_total)?;
        let event = without_area
            .checked_sub(original)
            .ok_or(ScoringError::DisplayPowerOverflow)?;
        with_area
            .checked_add(event)
            .ok_or(ScoringError::DisplayPowerOverflow)
    }

    /// Medley UI sums per-deck floats in double, casts to float, then truncates.
    pub fn medley_display(decks: &[Self; 3]) -> Result<i32, ScoringError> {
        Self::display_total_values(decks.map(|deck| deck.display_unrounded))
    }
    pub fn display_total_values(values: [f32; 3]) -> Result<i32, ScoringError> {
        let mut sum = 0.0_f64;
        for value in values {
            sum += nonnegative(value, "display power")? as f64;
        }
        display_integer(sum as f32)
    }
}

fn display_integer(value: f32) -> Result<i32, ScoringError> {
    if value.is_finite() && (0.0..2_147_483_648.0_f32).contains(&value) {
        Ok(value as i32)
    } else {
        Err(ScoringError::DisplayPowerOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_layout_power_matches_every_native_order() {
        let layouts = crate::hit_timing::layouts::physical_layouts();
        let mut state = 37u32;
        let mut saw_distinct_powers = false;
        for _ in 0..256 {
            let members = std::array::from_fn(|_| {
                std::array::from_fn(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    f32::from_bits(((134 + state % 12) << 23) | (state & 0x7fffff))
                })
            });
            let powers = layouts.iter().map(|order| {
                ScoringPower::from_member_parameters(&order.map(|i| members[i])).unwrap()
            });
            let max = powers.clone().max().unwrap();
            saw_distinct_powers |= powers.min().unwrap() != max;
            assert_eq!(ScoringPower::maximum_layout_power(&members), Ok(max));
        }
        assert!(saw_distinct_powers);
        for bad in [-1.0, f32::NAN, f32::INFINITY, f32::MAX] {
            let members = [[bad; 3]; 5];
            assert_eq!(
                ScoringPower::maximum_layout_power(&members),
                ScoringPower::from_member_parameters(&members)
            );
        }
    }

    #[test]
    fn retains_fractional_power_and_distinct_cache_keys() {
        let power = ScoringPower::from_member_parameters(&[[100_000.25; 3], [0.0; 3]]).unwrap();
        assert_eq!(power.value(), 300_000.75);
        assert_ne!(
            power.cache_key(),
            ScoringPower::new(300_000.0).unwrap().cache_key()
        );
        assert_eq!(ScoringPower::new(-0.0), ScoringPower::new(0.0));
        for value in [f32::NAN, f32::INFINITY, -1.0] {
            assert!(ScoringPower::new(value).is_err());
        }
    }
    #[test]
    fn preserves_native_component_addition_order() {
        let power = ScoringPower::from_member_parameters(&[[16_777_216.0, 1.0, 1.0]]).unwrap();
        assert_eq!(power.value(), 16_777_216.0);
        let power = ScoringPower::from_member_parameters(&[[1.0, 16_777_216.0, 1.0]]).unwrap();
        assert_eq!(power.value(), 16_777_218.0);
    }
    #[test]
    fn area_rates_match_original_arm32_before_division() {
        let values = [
            [10.0, 10.0, 1.1],
            [7.5, 22.5, 7.5],
            [10.0, 3.3, 10.0],
            [6.0, 18.0, 3.3],
            [18.0, 22.5, 3.3],
            [15.0, 18.0, 10.0],
            [22.5, 7.5, 7.5],
            [22.5, 10.0, 10.0],
        ];
        let result = AreaBonus::from_effects(&[], &values).unwrap();
        // Golden registers from DeckParamCalcUtility at 0x2291704/0x229172c.
        assert_eq!(
            result.rate.map(f32::to_bits),
            [1066317906, 1066343072, 1057417592]
        );
        assert_ne!(result.rate[2], 0.527_f32);
        assert!(AreaBonus::from_effects(&[], &[[f32::NAN; 3]]).is_err());
    }
    #[test]
    fn original_area_formula_and_medley_display_regressions() {
        let member =
            MemberPower::from_bonuses([10_001; 3], [0.0; 3], [0.25; 3], 0.0, [0.0; 3]).unwrap();
        assert_eq!(member.final_parameters(), [12_501.25; 3]);
        let deck = DeckPower::from_members(&[member]).unwrap();
        assert_eq!(deck.scoring().value(), 37_503.75);
        assert_eq!(deck.display_integer().unwrap(), 37_503);
        assert_eq!(
            DeckPower::display_total_values([300_000.75, 310_000.75, 320_000.75]).unwrap(),
            930_002
        );
        assert_eq!(
            DeckPower::display_total_values([16_777_216.0, 1.0, 1.0]).unwrap(),
            16_777_218
        );
    }

    #[test]
    fn challenge_deck_displays_integer_components_without_rounding_scoring_power() {
        let members = [
            ([14_324, 14_147, 14_639], 25.0, [0.0; 3]),
            ([14_113, 14_363, 14_639], 25.0, [0.0; 3]),
            ([17_731, 12_531, 12_848], 65.0, [0.0, 0.0, 6_424.0]),
            ([14_501, 13_833, 14_782], 25.0, [0.0; 3]),
            ([14_959, 14_324, 13_761], 25.0, [0.0; 3]),
        ]
        .map(|(original, event, extra)| {
            MemberPower::from_bonuses(
                original,
                [0.0; 3],
                [1.075_f32, 0.855_f32, 0.855_f32],
                event,
                extra,
            )
            .unwrap()
        });
        let deck = DeckPower::from_members(&members).unwrap();
        assert_eq!(deck.scoring().value(), 493_923.156_25);
        assert_eq!(
            deck.display_for_live(LiveVariant::ChallengeCp).unwrap(),
            493_922
        );
        for variant in [
            LiveVariant::Versus,
            LiveVariant::Festival,
            LiveVariant::Medley,
        ] {
            assert_eq!(deck.display_for_live(variant).unwrap(), 493_923);
        }
        for variant in [LiveVariant::Solo, LiveVariant::Cooperative] {
            assert_eq!(
                deck.display_for_live(variant).unwrap(),
                display_integer(deck.with_area_total).unwrap()
            );
        }
    }

    #[test]
    fn regular_live_display_matches_game_at_the_area_bonus_rounding_boundary() {
        let members = [
            [12_335, 11_995, 12_790],
            [13_752, 11_173, 12_180],
            [13_749, 12_132, 12_386],
            [13_815, 12_321, 11_251],
            [14_482, 11_505, 14_121],
        ]
        .map(|original| {
            MemberPower::from_bonuses(original, [0.0; 3], [1.075, 0.855, 0.855], 0.0, [0.0; 3])
                .unwrap()
        });
        let deck = DeckPower::from_members(&members).unwrap();
        assert_eq!(deck.display_for_live(LiveVariant::Solo).unwrap(), 367_415);
        assert_eq!(
            deck.display_for_live(LiveVariant::Cooperative).unwrap(),
            367_415
        );
        assert_eq!(deck.score_attack_display_integer().unwrap(), 367_414);
        assert_eq!(deck.scoring().value(), 367_415.156_25);
    }
}
