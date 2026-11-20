use super::*;

fn note(time: f64, window: NoteWindow) -> ReplayNote {
    ReplayNote {
        time,
        window,
        trigger: None,
    }
}

fn check(notes: &[ReplayNote]) -> BadBounds {
    let bad = BadBounds::new(notes).unwrap();
    for (index, n) in notes.iter().enumerate() {
        let expected = n.frame_domain(Judgement::Bad).unwrap();
        let (first, last) = bad.bounds[index];
        assert_eq!(
            (first, last),
            (*expected.first().unwrap(), *expected.last().unwrap()),
            "note {index}: {n:?}"
        );
        assert_eq!(
            expected.len(),
            (last - first + 1) as usize,
            "raw domain must be contiguous"
        );
    }
    bad
}

#[test]
fn moving_bad_endpoints_match_exhaustive_f32_at_boundaries_and_width_changes() {
    let mut notes = vec![];
    for frame in [0, 1, 2, 30, 119, 120, 121, 720, 50_000, 71_999, 72_000] {
        let origin = f64::from(frame) / f64::from(REPLAY_FPS);
        for units in [0., 0.5, 2.5, 5.5, 6.5, 7.5, 8.5, 9.] {
            let offset = units * (1.0_f32 / 60.0_f32);
            let offsets = if offset > 0. {
                [
                    f32::from_bits(offset.to_bits() - 1),
                    offset,
                    f32::from_bits(offset.to_bits() + 1),
                ]
            } else {
                [0.; 3]
            };
            for offset in offsets {
                for time in [origin - f64::from(offset), origin + f64::from(offset)] {
                    if !(0.0..=f64::from(MAX_SECONDS)).contains(&time) {
                        continue;
                    }
                    let times = if time > 0. {
                        [
                            f64::from_bits(time.to_bits() - 1),
                            time,
                            f64::from_bits(time.to_bits() + 1),
                        ]
                    } else {
                        [0.; 3]
                    };
                    for t in times.into_iter().filter(|t| *t <= f64::from(MAX_SECONDS)) {
                        for window in [NoteWindow::Standard, NoteWindow::WidenedEndpoint] {
                            notes.push(note(t, window));
                            notes.push(note(t, window));
                        }
                    }
                }
            }
        }
    }
    // Also force many small pointer advances and large gaps, in an unsorted input.
    for i in 0..8192 {
        let time = f64::from((i * 7919) % 72_001) / 120.;
        notes.push(note(
            time,
            if i % 2 == 0 {
                NoteWindow::Standard
            } else {
                NoteWindow::WidenedEndpoint
            },
        ));
    }
    notes.reverse();
    let bad = check(&notes);
    assert!(bad.same_time_reuses > 0);
    let exhaustive: u64 = notes
        .iter()
        .map(|n| {
            let (first, last) = n.frame_search_bounds().unwrap();
            u64::from(last - first + 1)
        })
        .sum();
    assert!(bad.judgement_probes < exhaustive);
    // Nominal integer-radius rounding would miss this f32-accepted endpoint.
    assert!(note(1., NoteWindow::Standard)
        .frame_domain(Judgement::Bad)
        .unwrap()
        .contains(&105));
}

#[test]
fn bad_sweep_keeps_inclusive_contacts_nested_explicit_domains_and_triggers() {
    let mut notes = vec![
        note(
            6.,
            NoteWindow::NativeGreen {
                first_frames: [1, 1, 1, 1],
                last_frame: 700,
            },
        ),
        note(1., NoteWindow::PinnedPerfect { frame: 20 }),
        note(2., NoteWindow::PinnedPerfect { frame: 40 }),
        note(
            3.,
            NoteWindow::NativeGreen {
                first_frames: [80, 80, 81, 82],
                last_frame: 100,
            },
        ),
        note(4., NoteWindow::PinnedPerfect { frame: 301 }),
        note(5., NoteWindow::PinnedPerfect { frame: 0 }),
        note(1.125, NoteWindow::WidenedEndpoint),
        note(0., NoteWindow::Standard),
        note(600., NoteWindow::WidenedEndpoint),
    ];
    for role in [crate::hit_timing::SlideTimeRole::Head, crate::hit_timing::SlideTimeRole::Middle,
        crate::hit_timing::SlideTimeRole::End, crate::hit_timing::SlideTimeRole::FlickEnd,
        crate::hit_timing::SlideTimeRole::DirectionalEnd] {
        for time in [0., 0.001, 0.18, 0.25, 5.999, 6., 599.999, 600.] {
            notes.push(note(time, NoteWindow::SlideTime { role, last_frame:None }));
        }
    }
    notes[4].trigger = Some(0);
    let bad = check(&notes);
    for ranges in [
        vec![],
        vec![(20, 20), (40, 80), (100, 300)],
        vec![(0, 0), (700, 720)],
        vec![(72_000, 72_000)],
    ] {
        let expected: Vec<_> = notes
            .iter()
            .map(|n| {
                n.trigger.is_some()
                    || n.frame_domain(Judgement::Bad)
                        .unwrap()
                        .iter()
                        .any(|&f| ranges.iter().any(|&(a, b)| a <= f && f <= b))
            })
            .collect();
        assert_eq!(bad.protected(&notes, &ranges), expected);
    }
}

#[test]
fn bad_bounds_reject_the_same_invalid_times_and_native_windows_as_frame_domains() {
    for n in [
        note(f64::NAN, NoteWindow::Standard),
        note(-0.01, NoteWindow::WidenedEndpoint),
        note(600.001, NoteWindow::Standard),
        note(
            1.,
            NoteWindow::NativeGreen {
                first_frames: [30, 20, 40, 50],
                last_frame: 60,
            },
        ),
        note(
            1.,
            NoteWindow::NativeGreen {
                first_frames: [1, 2, 3, 72_002],
                last_frame: 72_001,
            },
        ),
    ] {
        let old = n.frame_domain(Judgement::Bad).unwrap_err().to_string();
        let new = BadBounds::new(&[n]).err().unwrap().to_string();
        assert_eq!(new, old);
    }
}
