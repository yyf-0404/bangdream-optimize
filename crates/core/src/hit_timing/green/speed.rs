//! Exhaustive user-selected standard settings (1.00..12.00, step 0.01).
//! Button-history f32 drift is explicitly outside this selected domain.
//! Generate EVERY variant before selecting extrema witnesses. The representative
//! set is chart/chain-specific and never an a priori speed sample.
use super::{
    compact::{compile_chain, CompiledGreenChain, GreenChainNode},
    projection::{compile_projected_chain, GreenChainVariant, HiddenBridge},
    strict::compile_strict_chain,
};
use crate::hit_timing::{NoteWindow, ReplayError};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StandardSpeed(u16);
impl StandardSpeed {
    pub fn all() -> impl ExactSizeIterator<Item = Self> {
        (100..=1200).map(Self)
    }
    pub fn hundredths(self) -> u16 {
        self.0
    }
    pub fn value(self) -> f32 {
        f32::from(self.0) / 100.0f32
    }
}
#[derive(Clone, Debug)]
pub struct SpeedVariant {
    pub nodes: Vec<GreenChainNode>,
    pub bridges: Vec<HiddenBridge>,
}
impl SpeedVariant {
    fn borrowed(&self) -> GreenChainVariant<'_> {
        GreenChainVariant {
            nodes: &self.nodes,
            bridges: &self.bridges,
            adjust_b: 0,
        }
    }
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllSpeedChain {
    pub compiled: CompiledGreenChain,
    pub evaluated_settings: usize,
    /// Each value is a standard speed multiplied by 100. Witnesses need not be
    /// evenly spaced, contiguous or the same for two charts/chains.
    pub representative_settings: Vec<u16>,
}
fn cuts(w: NoteWindow) -> ([u32; 4], u32) {
    match w {
        NoteWindow::NativeGreen {
            first_frames,
            last_frame,
        } => (first_frames, last_frame),
        _ => unreachable!("validated by compile_chain"),
    }
}
fn same_model(a: &CompiledGreenChain, b: &CompiledGreenChain) -> bool {
    a.constraints == b.constraints
        && a.windows.len() == b.windows.len()
        && a.windows
            .iter()
            .zip(&b.windows)
            .all(|(a, b)| a.0 == b.0 && cuts(a.1) == cuts(b.1))
}
/// No early sampling or silent omission on error. For each primitive window/gap
/// extremum and every predecessor frame in the common domain, retain a speed
/// attaining the most restrictive value. A max/min over these witnesses equals
/// the max/min over the complete set; propagation and projection are unchanged.
/// Recompile both sets and assert exact model equality before returning.
pub fn compile_standard_speeds(
    mut generate: impl FnMut(StandardSpeed) -> Result<SpeedVariant, ReplayError>,
) -> Result<AllSpeedChain, ReplayError> {
    let variants = StandardSpeed::all()
        .map(&mut generate)
        .collect::<Result<Vec<_>, _>>()?;
    let sources: Vec<_> = variants.iter().map(SpeedVariant::borrowed).collect();
    let full = compile_projected_chain(&sources)?;
    let normalized = variants
        .iter()
        .map(|v| compile_chain(&v.nodes))
        .collect::<Result<Vec<_>, _>>()?;
    let common = compile_strict_chain(variants.iter().map(|v| v.nodes.as_slice()))?;
    let mut chosen = BTreeSet::from([0usize]);
    for node in 0..common.windows.len() {
        let mut maxima = [(0u32, 0usize); 4];
        let mut minimum = (u32::MAX, 0usize);
        for (speed, v) in normalized.iter().enumerate() {
            let (first, last) = cuts(v.windows[node].1);
            for grade in 0..4 {
                if first[grade] > maxima[grade].0 {
                    maxima[grade] = (first[grade], speed);
                }
            }
            if last < minimum.0 {
                minimum = (last, speed);
            }
        }
        chosen.extend(maxima.map(|p| p.1));
        chosen.insert(minimum.1);
    }
    for edge in 0..common.constraints.len() {
        let mut gap = (0u32, 0usize);
        for (speed, v) in normalized.iter().enumerate() {
            let g = v.constraints[edge].minimum_gap_frames;
            if g > gap.0 {
                gap = (g, speed);
            }
        }
        chosen.insert(gap.1);
        let (first, last) = cuts(common.windows[edge].1);
        for before in first[0]..=last {
            let mut maximum = (0u64, 0usize);
            for (speed, v) in variants.iter().enumerate() {
                let value = v.bridges[edge]
                    .earliest(before)
                    .map(u64::from)
                    .unwrap_or(u64::MAX);
                if value > maximum.0 {
                    maximum = (value, speed);
                }
            }
            chosen.insert(maximum.1);
        }
    }
    let reduced_sources: Vec<_> = chosen.iter().map(|&i| variants[i].borrowed()).collect();
    let reduced = compile_projected_chain(&reduced_sources)?;
    if !same_model(&full, &reduced) {
        return Err(ReplayError::Input(
            "standard-speed witnesses changed the common model",
        ));
    }
    Ok(AllSpeedChain {
        compiled: full,
        evaluated_settings: variants.len(),
        representative_settings: chosen.into_iter().map(|i| 100 + i as u16).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::green::{projection::HiddenGate, GreenRole};
    fn variant(speed: StandardSpeed) -> SpeedVariant {
        let code = speed.hundredths();
        let a = NoteWindow::NativeGreen {
            first_frames: [if code == 482 { 105 } else { 100 }; 4],
            last_frame: 130,
        };
        let b = NoteWindow::NativeGreen {
            first_frames: [104; 4],
            last_frame: if code == 1007 { 158 } else { 160 },
        };
        SpeedVariant {
            nodes: vec![
                GreenChainNode {
                    note_index: 4,
                    window: a,
                    minimum_gap_from_previous: 0,
                },
                GreenChainNode {
                    note_index: 9,
                    window: b,
                    minimum_gap_from_previous: 1,
                },
            ],
            bridges: vec![HiddenBridge {
                successor: GreenRole::HeldMiddle,
                hidden: vec![HiddenGate {
                    first_ready_frame: Some(if code == 1153 { 129 } else { 100 }),
                    expires_after_frame: Some(150),
                }],
            }],
        }
    }
    #[test]
    fn all_1101_settings_are_generated_before_independent_extrema_are_selected() {
        let mut visited = vec![];
        let result = compile_standard_speeds(|s| {
            visited.push(s.hundredths());
            Ok(variant(s))
        })
        .unwrap();
        assert_eq!(visited, (100..=1200).collect::<Vec<_>>());
        assert_eq!(result.evaluated_settings, 1101);
        assert_eq!(result.representative_settings, vec![100, 482, 1007, 1153]);
        let sources: Vec<_> = StandardSpeed::all().map(variant).collect();
        let direct = compile_projected_chain(
            &sources
                .iter()
                .map(SpeedVariant::borrowed)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(same_model(&direct, &result.compiled));
    }
    #[test]
    fn nominal_settings_do_not_accumulate_ui_rounding() {
        let all: Vec<_> = StandardSpeed::all().collect();
        assert_eq!(all.len(), 1101);
        assert_eq!(all[0].value(), 1.0);
        assert_eq!(all[1100].value(), 12.0);
        for pair in all.windows(2) {
            assert_eq!(pair[1].0, pair[0].0 + 1);
            assert!(pair[0].value() < pair[1].value());
        }
        assert_eq!(all[1001].value().to_bits(), 11.01f32.to_bits());
    }
    #[test]
    fn a_failure_in_an_unsampled_setting_is_never_dropped() {
        let mut visited = 0;
        let result = compile_standard_speeds(|s| {
            visited += 1;
            if s.0 == 1199 {
                Err(ReplayError::Input("native source failure"))
            } else {
                Ok(variant(s))
            }
        });
        assert!(result.is_err());
        assert_eq!(visited, 1100);
    }
}
