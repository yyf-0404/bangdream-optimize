use super::*;
fn row() -> Row {
    Row {
        domains: vec![],
        envelope: vec![],
        spans: vec![],
        references: vec![],
        relations: vec![
            Relation {
                before: 0,
                after: 1,
                gap: 1,
                earliest_after: None,
                strict_event_order: true,
            },
            Relation {
                before: 1,
                after: 2,
                gap: 1,
                earliest_after: None,
                strict_event_order: true,
            },
        ],
        incident: vec![vec![0], vec![0, 1], vec![1]],
    }
}
#[test]
fn only_times_without_an_unsettled_neighbour_are_discarded() {
    let row = row();
    let mut key = Key {
        marks: vec![121, 131, 0],
        bindings: vec![4],
    };
    forget_closed_timestamps(&row, &[0, 1, 2], &[false; 3], 1, &mut key);
    assert_eq!(key.marks, [1, 131, 0]);
    assert_eq!(key.bindings, [4]);
    // The middle timestamp is still available for checking the last edge.
    assert!(row.relations[1].allows_frames(key.marks[1] - 1, 140));
    key.marks[2] = 141;
    forget_closed_timestamps(&row, &[0, 1, 2], &[false; 3], 2, &mut key);
    assert_eq!(key.marks, [1, 1, 1]);
}
#[test]
fn future_not_yet_active_neighbours_keep_their_predecessor_time() {
    let row = row();
    let mut key = Key {
        marks: vec![121, 131],
        bindings: vec![],
    };
    forget_closed_timestamps(&row, &[0, 1], &[false; 3], 1, &mut key);
    assert_eq!(key.marks, [1, 131]);
    // A retired endpoint already completed all its checks.
    let mut key = Key {
        marks: vec![131, 141],
        bindings: vec![],
    };
    forget_closed_timestamps(&row, &[1, 2], &[true, false, false], 2, &mut key);
    assert_eq!(key.marks, [1, 1]);
}

#[test]
fn completed_frame_saturates_strict_one_frame_gaps() {
    let row = row();
    let mut key = Key {
        marks: vec![121, 0],
        bindings: vec![7],
    };
    saturate_completed_frame_timestamps(&row, &[0, 1], &[false; 3], 120, &mut key);
    assert_eq!(key.marks, [1, 0]);
    assert_eq!(key.bindings, [7]);

    // An earlier frame cannot yet discharge a hit from frame 120.
    key.marks[0] = 121;
    saturate_completed_frame_timestamps(&row, &[0, 1], &[false; 3], 119, &mut key);
    assert_eq!(key.marks, [121, 0]);
}

#[test]
fn completed_frame_keeps_a_timestamp_until_every_gap_is_satisfied() {
    let mut row = row();
    row.relations[0].gap = 2;
    let mut key = Key {
        marks: vec![121, 0],
        bindings: vec![],
    };
    saturate_completed_frame_timestamps(&row, &[0, 1], &[false; 3], 120, &mut key);
    assert_eq!(key.marks, [121, 0]);
    saturate_completed_frame_timestamps(&row, &[0, 1], &[false; 3], 121, &mut key);
    assert_eq!(key.marks, [1, 0]);

    row.relations[0].strict_event_order = false;
    key.marks[0] = 121;
    saturate_completed_frame_timestamps(&row, &[0, 1], &[false; 3], 121, &mut key);
    assert_eq!(key.marks, [121, 0]);
}
