use super::*;
use serde_json::json;

fn reference(clocks: &[NativeClock], nodes: &[TimeoutNode]) -> Result<(Vec<u32>, usize), ReplayError> {
    let horizon = clocks.len() as u32 - 1;
    let mut checks = 0;
    let mut result = Vec::new();
    for node in nodes {
        let mut last = horizon;
        for frame in node.first_clock..=horizon {
            checks += 1;
            if clocks[frame as usize].late_seconds(node.absolute_pos)? > MISS_SECOND_INTERVAL {
                last = frame - 1;
                break;
            }
        }
        result.push(last);
    }
    Ok((result, checks))
}

fn clock(pos: f32, bpm: f32) -> NativeClock {
    NativeClock { adjusted_music_pos: pos, current_bpm: bpm, resolution: 192 }
}

#[test]
fn timeout_queue_matches_per_node_scan_with_bpm_reversals_and_delayed_activation() {
    for case in 0..128u32 {
        let clocks: Vec<_> = (0..=512).map(|f| {
            // Some traces deliberately reverse C too: timeout queue ordering
            // relies only on the ordering of A at a fixed frame.
            let pos = if case % 3 == 0 { (f % 91) as f32 } else { f as f32 * 0.75 };
            let bpm = [0.001, 75., 150., 30000.][((f / 13 + case) % 4) as usize];
            clock(pos, bpm)
        }).collect();
        let nodes: Vec<_> = (0..32).map(|i| TimeoutNode {
            first_clock: 1 + (i / 3) * (case % 29),
            absolute_pos: (i / 2) as i32 * 11,
        }).collect();
        let expected = reference(&clocks, &nodes).unwrap().0;
        let mut checked = 0;
        let actual = timeout_last_frames_counted(&clocks, &nodes, || checked += 1).unwrap();
        assert_eq!(actual, expected, "case {case}");
        assert!(checked <= 2 * 512 + nodes.len());
    }
    let clocks = vec![clock(16_777_216., 185.); 10];
    let nodes: Vec<_> = (0..16).map(|i| TimeoutNode {
        first_clock: i + 1, absolute_pos: 16_777_210 + i as i32,
    }).collect();
    assert_eq!(timeout_last_frames(&clocks, &nodes).unwrap(), reference(&clocks, &nodes).unwrap().0);
    assert_eq!(timeout_last_frames(&clocks, &[]).unwrap(), Vec::<u32>::new());
}

#[test]
fn timeout_queue_retains_arithmetic_error_for_skipped_negative_extreme() {
    let clocks = vec![clock(0., 1e-30); 3];
    let nodes = [TimeoutNode { first_clock: 1, absolute_pos: 0 },
        TimeoutNode { first_clock: 1, absolute_pos: i32::MAX }];
    assert_eq!(clocks[1].late_seconds(0).unwrap(), 0.);
    assert!(matches!(reference(&clocks, &nodes), Err(ReplayError::Input("native green clock overflow"))));
    assert!(matches!(timeout_last_frames(&clocks, &nodes), Err(ReplayError::Input("native green clock overflow"))));
}

#[test]
fn timeout_scan_work_is_bounded_when_all_nodes_survive_the_horizon() {
    let clocks: Vec<_> = (0..=72000).map(|f| clock(f as f32 / 1000., 150.)).collect();
    let nodes: Vec<_> = (0..1000).map(|i| TimeoutNode { first_clock: 1, absolute_pos: 1000 + i }).collect();
    let mut checks = 0;
    assert_eq!(timeout_last_frames_counted(&clocks, &nodes, || checks += 1).unwrap(), vec![72000; 1000]);
    assert_eq!(checks, 144000);
    // The old loop must visit every pair here: 72,000 * 1,000.
}

#[test]
fn midpoint_binary_search_matches_literal_predicates_at_float_boundaries() {
    let mut positions: Vec<_> = (0..400).flat_map(|i| [i as f32, i as f32]).collect();
    for centre in [16_777_216f32, 536_870_912., 1_073_741_824.] {
        positions.extend((-8i32..=8).map(|d| f32::from_bits((i64::from(centre.to_bits()) + i64::from(d)) as u32)));
    }
    let clocks: Vec<_> = positions.iter().map(|&c| clock(c, 150.)).collect();
    for (current, next) in [(0.,0.), (0.,1.), (100.,101.), (100.,300.),
        (16_777_216.,16_777_218.), (127.,1_073_741_824.)] {
        for head in [false, true] {
            for first in (0..clocks.len() as u32).step_by(17) {
                for last in [first.saturating_sub(1), first, clocks.len() as u32 - 1] {
                    let expected = (first..=last).find(|&f| {
                        let c = clocks[f as usize].adjusted_music_pos;
                        let (until, since) = (next - c, c - current);
                        if head { until <= 0. || since > until } else { until > 0. && since > until }
                    });
                    let mut checks = 0;
                    let actual = midpoint_expiry_counted(&clocks, first, last, current, next, head, || checks += 1);
                    assert_eq!(actual, expected, "A={current}, B={next}, head={head}, first={first}, last={last}");
                    assert!(checks <= 12);
                }
            }
        }
    }
    // Exactly equal rounded differences must not become a crossing merely
    // because the real-number midpoint is slightly smaller than this C.
    let c = 536_870_976.;
    assert_eq!(c - 127.0f32, 1_073_741_824.0f32 - c);
    assert_eq!(midpoint_expiry(&[clock(c, 150.)], 0, 0, 127., 1_073_741_824., true), None);
    let clocks = vec![clock(300., 150.); 72001];
    for head in [false, true] {
        let mut checks = 0;
        assert_eq!(midpoint_expiry_counted(&clocks, 2, 72000, 100., 300., head, || checks += 1),
            if head { Some(2) } else { None });
        assert_eq!(checks, 1);
    }
}

#[test]
#[ignore = "isolated release benchmark; run explicitly with --ignored --nocapture"]
fn benchmark_shared_timeout_walk() {
    use std::{hint::black_box, time::Instant};
    let clocks: Vec<_> = (0..=72000).map(|f| clock(f as f32 / 1000., 150.)).collect();
    let nodes: Vec<_> = (0..1000).map(|i| TimeoutNode { first_clock: 1, absolute_pos: 1000 + i }).collect();
    let (expected, old_checks) = reference(&clocks, &nodes).unwrap();
    let mut new_checks = 0;
    assert_eq!(timeout_last_frames_counted(&clocks, &nodes, || new_checks += 1).unwrap(), expected);
    let (mut old_ms, mut new_ms) = (vec![], vec![]);
    for _ in 0..5 {
        let start = Instant::now();
        black_box(reference(black_box(&clocks), black_box(&nodes)).unwrap());
        old_ms.push(start.elapsed().as_secs_f64()*1000.);
        let start = Instant::now();
        black_box(timeout_last_frames(black_box(&clocks), black_box(&nodes)).unwrap());
        new_ms.push(start.elapsed().as_secs_f64()*1000.);
    }
    println!("{}",json!({"benchmark":"timeout-synthetic-horizon","frames":72000,"nodes":1000,
        "oldChecks":old_checks,"newChecks":new_checks,"beforeMs":old_ms,"afterMs":new_ms}));
}
