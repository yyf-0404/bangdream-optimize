//! Integer growth from the nominal decimal rates stored in a profile.
//! Server append parameters are integers. A binary approximation such as
//! 5500 * 0.018 = 98.99999999999999 must not lose one point before scoring.
use super::{CharacterBonusConfig, StatValue};

pub(super) fn apply_character_bonuses(
    base: StatValue,
    bonus: &CharacterBonusConfig,
) -> Result<StatValue, &'static str> {
    let original = [base.performance, base.technique, base.visual];
    let potential = [
        bonus.potential.performance,
        bonus.potential.technique,
        bonus.potential.visual,
    ];
    let task = [
        bonus.character_task.performance,
        bonus.character_task.technique,
        bonus.character_task.visual,
    ];
    let mut result = [0_u32; 3];
    for i in 0..3 {
        let value = original[i];
        if !value.is_finite() || !(0.0..4_294_967_296.0).contains(&value) || value.fract() != 0.0 {
            return Err("base parameter must be a nonnegative u32 integer");
        }
        let base = value as u32;
        // Both bonuses use the same original integer, never the other's result.
        let potential = decimal_bonus(base, potential[i])?;
        let task = decimal_bonus(base, task[i])?;
        result[i] = base
            .checked_add(potential)
            .and_then(|v| v.checked_add(task))
            .ok_or("grown parameter exceeds u32")?;
    }
    Ok(StatValue {
        performance: f64::from(result[0]),
        technique: f64::from(result[1]),
        visual: f64::from(result[2]),
    })
}

/// Interpret the shortest round-trip decimal of a finite rate exactly.
/// This is an explicit input convention, not a claim about the unknown server
/// implementation. It also preserves a declared value immediately below a
/// decimal boundary; no epsilon or snapping to a percentage grid is used.
fn decimal_bonus(base: u32, rate: f64) -> Result<u32, &'static str> {
    if !rate.is_finite() || rate < 0.0 {
        return Err("character rate must be finite and nonnegative");
    }
    if base == 0 || rate == 0.0 {
        return Ok(0);
    }
    // LowerExp uses the shortest round-trip significand (at most 17 digits).
    let decimal = format!("{rate:e}");
    let (mantissa, exponent) = decimal.split_once('e').ok_or("invalid decimal rate")?;
    let exponent = exponent
        .parse::<i32>()
        .map_err(|_| "invalid decimal exponent")?;
    let fraction_digits = mantissa.split_once('.').map_or(0, |(_, s)| s.len() as i32);
    let significand = mantissa
        .bytes()
        .filter(|&b| b != b'.')
        .try_fold(0_u128, |n, b| {
            if !b.is_ascii_digit() {
                return None;
            }
            n.checked_mul(10)?.checked_add(u128::from(b - b'0'))
        })
        .ok_or("invalid decimal significand")?;
    let numerator = u128::from(base)
        .checked_mul(significand)
        .ok_or("growth product overflow")?;
    let scale = exponent - fraction_digits;
    let integer = if scale >= 0 {
        let power = 10_u128
            .checked_pow(scale as u32)
            .ok_or("growth product exceeds u32")?;
        numerator
            .checked_mul(power)
            .ok_or("growth product exceeds u32")?
    } else if scale <= -39 {
        // The checked numerator is <= u128::MAX < 10^39.
        0
    } else {
        numerator / 10_u128.pow((-scale) as u32)
    };
    u32::try_from(integer).map_err(|_| "character bonus exceeds u32")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::schema::StatRate;

    #[test]
    fn archived_card_346_task_bonus_keeps_the_integer_boundary() {
        assert_eq!(5500.0_f64 * 0.018, 98.99999999999999);
        // The archived server append field is 99, not 98.
        assert_eq!(decimal_bonus(5500, 0.018), Ok(99));
        assert_eq!(
            decimal_bonus(5500, f64::from_bits(0.018_f64.to_bits() - 1)),
            Ok(98)
        );
    }

    #[test]
    fn independent_bonus_truncations_do_not_compound_or_merge() {
        let bonus = CharacterBonusConfig {
            potential: StatRate {
                performance: 0.018,
                technique: 0.018,
                visual: 0.018,
            },
            character_task: StatRate {
                performance: 0.018,
                technique: 0.018,
                visual: 0.018,
            },
        };
        assert_eq!(
            apply_character_bonuses(
                StatValue {
                    performance: 5500.,
                    technique: 51.,
                    visual: 0.
                },
                &bonus
            ),
            Ok(StatValue {
                performance: 5698.,
                technique: 51.,
                visual: 0.
            })
        );
    }

    #[test]
    fn decimal_growth_covers_exponents_and_rejects_invalid_numeric_inputs() {
        assert_eq!(decimal_bonus(u32::MAX, 1e-300), Ok(0));
        assert_eq!(decimal_bonus(1, 1e9), Ok(1_000_000_000));
        assert_eq!(decimal_bonus(0, 1e300), Ok(0));
        assert!(decimal_bonus(1, 1e300).is_err());
        assert!(decimal_bonus(1, f64::INFINITY).is_err());
        assert!(decimal_bonus(0, -0.01).is_err());
        let zero = StatRate {
            performance: 0.,
            technique: 0.,
            visual: 0.,
        };
        let bonus = CharacterBonusConfig {
            potential: zero,
            character_task: zero,
        };
        for value in [-1., 1.5, f64::NAN, 4_294_967_296.] {
            assert!(apply_character_bonuses(
                StatValue {
                    performance: value,
                    technique: 0.,
                    visual: 0.
                },
                &bonus
            )
            .is_err());
        }
        let full = CharacterBonusConfig {
            potential: StatRate {
                performance: 1.,
                technique: 0.,
                visual: 0.,
            },
            character_task: zero,
        };
        assert!(apply_character_bonuses(
            StatValue {
                performance: f64::from(u32::MAX),
                technique: 0.,
                visual: 0.
            },
            &full
        )
        .is_err());
    }
}
