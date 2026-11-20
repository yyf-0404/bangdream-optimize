use super::*;
use crate::hit_timing::NoteConstraint;

fn mixed(skill: ReplaySkill) -> ReplayInput {
    let mut r = input(
        &[0.1, 0.3, 0.4],
        vec![ReplaySkill {
            duration: 0.001,
            ..skill
        }],
        0,
    );
    r.initial_combo = 1000;
    r.hit_frames = vec![12, 42, 45];
    let routing = r.native_time_routing.as_mut().unwrap();
    for (i, first, last) in [(1, 40, 44), (2, 43, 47)] {
        let target = routing.targets[i].as_mut().unwrap();
        target.active_from_frame = first;
        target.active_until_frame = last;
    }
    r
}

#[test]
fn independent_outside_note_splits_a_mixed_component_without_changing_native_score() {
    for profile in profiles() {
        let mut request = mixed(profile);
        assert_eq!(local::protected(&request).unwrap(), vec![true, true, false]);
        for medley in [false, true] {
            request.medley_combo = medley;
            for power in [2., 300040.75, f32::from_bits(300040.75_f32.to_bits() + 1)] {
                request.power = power;
                let result = local_geometry::compare(&request, brute(&request));
                assert_eq!(result.stats.outside_pinned_notes, 1);
                assert_eq!(result.stats.collapsed_notes, 1);
                assert_eq!(
                    result
                        .result
                        .trace
                        .iter()
                        .find(|h| h.note_index == 2)
                        .unwrap()
                        .frame,
                    45
                );
            }
        }
    }
}

#[test]
fn outside_pin_keeps_combo_boundary_order_choices() {
    let mut request = mixed(profiles()[0].clone());
    request.initial_combo = 18;
    request.hit_frames[1] = 46;
    let target = request.native_time_routing.as_mut().unwrap().targets[1]
        .as_mut()
        .unwrap();
    target.active_from_frame = 46;
    target.active_until_frame = 46;
    let expected = brute(&request);
    let result = local_geometry::compare(&request, expected);
    assert_eq!(result.stats.outside_pinned_notes, 0);
    // Great at Combo 20, then Perfect at Combo 21, beats the baseline order.
    assert!(expected > replay(&request).unwrap().total);
    assert!(
        result
            .result
            .trace
            .iter()
            .find(|h| h.note_index == 2)
            .unwrap()
            .frame
            >= 46
    );
}

#[test]
fn outside_pin_keeps_fever_boundary_choices() {
    let mut request = mixed(profiles()[0].clone());
    request.fever_frames = Some([44, 47]);
    let result = local_geometry::compare(&request, brute(&request));
    assert_eq!(result.stats.outside_pinned_notes, 0);
}

#[test]
fn outside_pin_preserves_other_roots_early_consumption_requirements() {
    let mut request = mixed(profiles()[0].clone());
    request.native_time_routing.as_mut().unwrap().targets[2]
        .as_mut()
        .unwrap()
        .button = 1;
    let mut blocked = request.clone();
    blocked.hit_frames[1] = 43;
    assert!(replay(&blocked).is_err());
    let result = local_geometry::compare(&request, brute(&request));
    assert_eq!(result.stats.outside_pinned_notes, 0);
}

#[test]
fn outside_pin_keeps_retained_time_constraints() {
    let mut request = mixed(profiles()[0].clone());
    request.constraints.push(NoteConstraint {
        before: 1,
        after: 2,
        minimum_gap_frames: 2,
        require_dispatch_order: false,
        earliest_after: None,
    });
    let result = local_geometry::compare(&request, brute(&request));
    assert_eq!(result.stats.outside_pinned_notes, 0);
}

#[test]
fn outside_pin_discharges_gaps_satisfied_for_every_neighbor_frame() {
    for dispatch in [false, true] {
        let mut request = mixed(profiles()[0].clone());
        request.constraints.push(NoteConstraint {
            before: 1,
            after: 2,
            minimum_gap_frames: 1,
            require_dispatch_order: dispatch,
            earliest_after: None,
        });
        let result = local_geometry::compare(&request, brute(&request));
        assert_eq!(result.stats.outside_pinned_notes, 1);
        assert_eq!(result.stats.collapsed_notes, 1);
    }
}
