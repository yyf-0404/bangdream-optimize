//! Exact dyadic arithmetic audits the f64 coefficient path, not native casts.
use super::*;
use super::super::geometry::{Action, ActionHits, Edge, Graph, Layer};
use crate::hit_timing::{NoteWindow, ReplayNote};

// These fixtures' exact products/sums fit in signed 128 bits at this scale.
// Conversions below check that no fractional or high integer bits are lost.
const SCALE: i32 = 112;

fn shifted(mantissa: u128, exponent: i32) -> i128 {
    let integer = if exponent >= 0 {
        mantissa.checked_mul(1_u128.checked_shl(exponent as u32).unwrap()).unwrap()
    } else {
        let shift = (-exponent) as u32;
        let mask = 1_u128.checked_shl(shift).unwrap() - 1;
        assert_eq!(mantissa & mask, 0, "dyadic oracle scale must be exact");
        mantissa >> shift
    };
    i128::try_from(integer).unwrap()
}

fn scaled(value: f64) -> i128 {
    if value == 0. { return 0; }
    let bits = value.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    assert!((1..0x7ff).contains(&exponent));
    let mantissa = u128::from((1_u64 << 52) | (bits & ((1_u64 << 52)-1)));
    let value = shifted(mantissa, exponent-1023-52+SCALE);
    if bits >> 63 != 0 { -value } else { value }
}

fn exact_unit(input: &ReplayInput, perfect: bool, rank: usize, fever: bool, multiplier: f32) -> i128 {
    let count = input.notes.len();
    assert!(count.is_power_of_two());
    let operands = [
        1_f32 + (input.play_level-5) as f32 * 0.01_f32,
        if perfect { Judgement::Perfect } else { Judgement::Great }.multiplier(),
        input.combo_multiplier(input.initial_combo + rank as u32),
        (if fever { 2_f32 } else { 1_f32 }) * multiplier,
    ];
    let mut numerator = 3_u128;
    let mut exponent = -(count.trailing_zeros() as i32);
    for value in operands {
        if value == 0. { return 0; }
        let bits = value.to_bits();
        let e = (bits >> 23) as i32;
        assert!((1..255).contains(&e));
        numerator = numerator.checked_mul(u128::from((1 << 23) | (bits & 0x7fffff))).unwrap();
        exponent += e-127-23;
    }
    shifted(numerator, exponent+SCALE)
}

fn skill(score: ScoreEffect) -> ReplaySkill {
    ReplaySkill { duration: 0.5, correction: None, score }
}

fn input(count: usize, level: i32, combo: u32, fever: bool, skill: &ReplaySkill) -> ReplayInput {
    let frames: Vec<_> = (1..=count).map(|i| i as u32 * 4).collect();
    ReplayInput { power: 0., play_level: level, fps: 120,
        notes: frames.iter().enumerate().map(|(i,&frame)| ReplayNote {
            time: f64::from(frame)/120., trigger: (i==0).then_some(0),
            window: NoteWindow::PinnedPerfect { frame } }).collect(),
        hit_frames: frames, hit_order: vec![], skills: vec![skill.clone()],
        initial_combo: combo, medley_combo: combo > 1000,
        fever_frames: fever.then_some([0,count as u32*4+1]),
        constraints: vec![], native_time_routing: None }
}

#[test]
fn exact_dyadics_cover_short_late_prefix_differences_combo_splits_and_absorbing_runs() {
    for effect in [
        ScoreEffect::Crescendo { percent: 100., step: 0.001, cap: 10000 },
        ScoreEffect::Crescendo { percent: 100., step: 0.5, cap: 102 },
    ] {
        let skill = skill(effect);
        for count in [64,512,4096] { for level in [1,5,27,100] {
            for combo in [49,2990] { for fever in [false,true] {
                let input = input(count,level,combo,fever,&skill);
                let machine = Machine::new(&skill,count).unwrap();
                let k = machine.values.len();
                let mut starts = vec![0,k/2,k.saturating_sub(3),k-1];
                starts.sort_unstable(); starts.dedup();
                for start in starts {
                    let length = (count-start).min(13);
                    if length < 2 { continue; }
                    let temporary = Costs::net(&input,&machine,0.).unwrap();
                    let mut references = vec![0.;count];
                    let mut expected = 0_i128;
                    let mut state = start;
                    for j in 0..length {
                        let rank = start+1+j;
                        let multiplier = machine.values[state][1];
                        let rounded = temporary.unit(true,temporary.combo(rank),fever,multiplier)
                            .unwrap().coefficient;
                        let reference = if j%2==0 { rounded.next_up() } else { rounded.next_down() };
                        references[rank-1] = reference;
                        expected += exact_unit(&input,true,rank,fever,multiplier)-scaled(reference);
                        state = machine.next[state][1];
                    }
                    let total = references.iter().fold(0_f64,|a,&b| (a+b).next_up());
                    let mut costs = Costs::net(&input,&machine,total).unwrap();
                    let factor = Factor { perfect: true, inside: true, slot: 1, fever,
                        reference: references[start..start+length].iter().sum(), rank: start+1 };
                    let (actual_state,value) = costs.run(factor,start,length).unwrap();
                    assert_eq!(actual_state,state);
                    let allowance = scaled(costs.coefficient_error());
                    assert!((scaled(value.coefficient)-expected).abs() <= allowance,
                        "N={count} K={k} level={level} combo={combo} start={start} length={length}");
                    // A reused prefix table keeps the identical full operands.
                    let again = costs.run(factor,start,length).unwrap();
                    assert_eq!(again.0,actual_state);
                    assert_eq!(again.1.coefficient.to_bits(),value.coefficient.to_bits());
                }
            }}
        }}
    }
}

#[test]
fn continuous_dp_max_encloses_exact_signed_paths_without_native_state_reductions() {
    for effect in [
        ScoreEffect::Continuous { high: 140., low: 60. },
        ScoreEffect::PerfectOrZero { percent: 140. },
        ScoreEffect::PerfectOrHalf { percent: 140. },
        ScoreEffect::Crescendo { percent: 130., step: 0.5, cap: 130 },
    ] {
        let skill = skill(effect);
        for level in [1,5,27,100] { for combo in [49,2999] { for fever in [false,true] {
            let input = input(64,level,combo,fever,&skill);
            let machine = Machine::new(&skill,64).unwrap();
            let temporary = Costs::net(&input,&machine,0.).unwrap();
            let reference = std::array::from_fn::<_,2,_>(|i| temporary
                .unit(true,temporary.combo(i+1),fever,machine.values[0][1])
                .unwrap().coefficient.next_up());
            let total = reference.iter().fold(0_f64,|a,&b| (a+b).next_up());
            let mut actions = vec![];
            let mut layers = vec![];
            for note in 0..2 {
                let mut edges = vec![];
                for perfect in [false,true] {
                    let action = actions.len();
                    actions.push(Action { factor: Factor { perfect, inside: true, slot: 1,
                        fever, reference: reference[note], rank: note+1 },
                        hits: ActionHits::Event([(note,if perfect { note as u32*4+1 }
                            else { note as u32*4+2 })]) });
                    edges.push(Edge { from:0,to:0,action:Some(action) });
                }
                layers.push(Layer { count:1,edges:edges.into() });
            }
            let tail = actions.len();
            actions.push(Action { factor: Factor { perfect:true,inside:false,slot:0,
                fever,reference:0.,rank:3 },
                hits: (2..64).map(|i| (i,i as u32*4+1)).collect::<Vec<_>>().into() });
            layers.push(Layer { count:1,edges:vec![Edge {from:0,to:0,action:Some(tail)}].into() });
            let graph = Graph { layers, actions:actions.into(), note_count:64,
                edge_count:5, temporal:vec![], touches:vec![] };
            let mut real_max = i128::MIN;
            for first in [false,true] { for second in [false,true] {
                let mut state = 0;
                let mut value = 0;
                for (note,perfect) in [first,second].into_iter().enumerate() {
                    let grade = usize::from(perfect);
                    value += exact_unit(&input,perfect,note+1,fever,machine.values[state][grade])
                        -scaled(reference[note]);
                    state = machine.next[state][grade];
                }
                real_max = real_max.max(value);
            }}
            for compact in [false,true] {
                let mut costs = Costs::net(&input,&machine,total).unwrap();
                let mut stats = WindowStats::default();
                let options = WindowOptions { compact_value_states:compact,
                    incremental_touch:true, max_states:usize::MAX,max_transitions:u64::MAX,
                    ..WindowOptions::default() };
                let value = super::super::solve::evaluate_value(&graph,&mut costs,
                    WindowObjective::Continuous,options,&mut stats).unwrap().unwrap().unwrap();
                let allowance = scaled(costs.coefficient_error());
                assert!((scaled(value.coefficient)-real_max).abs() <= allowance);
                assert!(real_max <= scaled((value.coefficient+costs.coefficient_error()).next_up()));
                assert_eq!(stats.dominated_value_states,0);
                assert_eq!(stats.temporal_skipped_intervals,0);
            }
        }}}
    }
}
