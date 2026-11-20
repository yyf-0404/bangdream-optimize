use super::*;
use serde_json::json;

// Literal pre-optimization loop: checks every remaining command and uses
// Vec::remove + i+=1. It intentionally does not share the new event iterator.
fn reference(map: &mut TempoMap, last: u32) -> Result<(Vec<NativeClock>, usize, usize), DataError> {
    let mut progress = BarProgress::default();
    let mut bpm = map.0[0].bpm as f32;
    let mut pending: Vec<_> = (1..map.0.len()).collect();
    let mut clocks = vec![NativeClock { adjusted_music_pos: 0., current_bpm: bpm, resolution: 192 }];
    let (mut checks, mut moves) = (0, 0);
    for frame in 1..=last {
        progress.update_120(bpm)?;
        let mut i = 0;
        while i < pending.len() {
            checks += 1;
            let command = &mut map.0[pending[i]];
            if progress.reached(command.tick / 192, command.tick % 192, 192)? {
                bpm = command.bpm as f32;
                command.applied_frame = Some(frame);
                moves += pending.len() - i - 1;
                pending.remove(i);
            }
            i += 1;
        }
        clocks.push(NativeClock { adjusted_music_pos: progress.absolute_position()?, current_bpm: bpm, resolution: 192 });
    }
    Ok((clocks, checks, moves))
}

fn bits(clocks: &[NativeClock]) -> Vec<(u32, u32, i32)> {
    clocks.iter().map(|c| (c.adjusted_music_pos.to_bits(), c.current_bpm.to_bits(), c.resolution)).collect()
}

fn compare(events: &[Value], last: u32) -> (usize, usize, usize) {
    let mut old = TempoMap::new(events).unwrap();
    let mut new = TempoMap::new(events).unwrap();
    let (expected, old_checks, moves) = reference(&mut old, last).unwrap();
    let mut new_checks = 0;
    let actual = new.music_clocks_counted(last, || new_checks += 1).unwrap();
    assert_eq!(bits(&actual), bits(&expected));
    assert_eq!(serde_json::to_value(&new.0).unwrap(), serde_json::to_value(&old.0).unwrap());
    assert!(new_checks <= last as usize + events.len() - 1);
    assert!(actual.windows(2).all(|w| w[0].adjusted_music_pos <= w[1].adjusted_music_pos));
    (old_checks, new_checks, moves)
}

#[test]
fn ordered_commands_match_literal_removal_loop_bit_for_bit() {
    let mut seed = 0x371b_97a1u32;
    for case in 0..96 {
        let initial = [75., 185., 600., 30_000.][case % 4];
        let mut events = vec![json!({"type":"BPM","beat":0,"bpm":initial})];
        let mut tick = 0;
        for _ in 0..(case % 32 + 1) {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            tick += 1 + (seed % 37);
            let bpm = [75., 150., 185., 600., 30_000.][(seed as usize >> 16) % 5];
            events.push(json!({"type":"BPM","beat":f64::from(tick)/48.,"bpm":bpm}));
        }
        compare(&events, 512);
    }
    // Includes an unreached suffix, repeated rates, a delayed skipped command,
    // crossing many bars in one update, and the complete supported horizon.
    for events in [
        json!([{"type":"BPM","beat":0,"bpm":185}]),
        json!([{"type":"BPM","beat":0,"bpm":150}, {"type":"BPM","beat":1500,"bpm":185}]),
        json!([{"type":"BPM","beat":0,"bpm":1e9}, {"type":"BPM","beat":1,"bpm":150}, {"type":"BPM","beat":2,"bpm":300}, {"type":"BPM","beat":3,"bpm":185}]),
        json!([{"type":"BPM","beat":0,"bpm":120}, {"type":"BPM","beat":4,"bpm":0.001}]),
    ] { compare(events.as_array().unwrap(), 72000); }
}

#[test]
fn unreached_bpm_suffix_is_checked_once_per_frame() {
    let mut events = vec![json!({"type":"BPM","beat":0,"bpm":150})];
    for i in 0..1000 {
        events.push(json!({"type":"BPM","beat":(60000.+f64::from(i))/48.,"bpm":150}));
    }
    let (before, after, moves) = compare(&events, 2000);
    assert_eq!((before, after, moves), (2_000_000, 2000, 0));
}

#[test]
#[ignore = "isolated release benchmark; run explicitly with --ignored --nocapture"]
fn benchmark_bpm_event_walk() {
    use std::{hint::black_box, time::Instant};
    let mut events = vec![json!({"type":"BPM","beat":0,"bpm":150})];
    for i in 0..1000 {
        events.push(json!({"type":"BPM","beat":(60000.+10.*f64::from(i))/48.,"bpm":150}));
    }
    let (old_checks, new_checks, moved_indices) = compare(&events, 72000);
    let (mut old_ms, mut new_ms) = (vec![], vec![]);
    for _ in 0..5 {
        let mut old = TempoMap::new(&events).unwrap();
        let start = Instant::now();
        black_box(reference(&mut old, 72000).unwrap());
        old_ms.push(start.elapsed().as_secs_f64()*1000.);
        let mut new = TempoMap::new(&events).unwrap();
        let start = Instant::now();
        black_box(new.music_clocks(72000).unwrap());
        new_ms.push(start.elapsed().as_secs_f64()*1000.);
    }
    println!("{}", json!({"benchmark":"bpm-events","frames":72000,"events":1000,
        "oldChecks":old_checks,"newChecks":new_checks,"oldMovedIndices":moved_indices,
        "beforeMs":old_ms,"afterMs":new_ms}));
}
