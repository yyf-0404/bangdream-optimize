use super::*;

fn assert_graph(a: &Graph, b: &Graph) {
    assert_eq!(a.edge_count, b.edge_count);
    assert_eq!(a.layers.len(), b.layers.len());
    assert_eq!(a.actions.len(), b.actions.len());
    assert_eq!(a.actions.capacity(), b.actions.capacity());
    for (a, b) in a.layers.iter().zip(&b.layers) {
        assert_eq!(a.count, b.count);
        assert_eq!(a.edges.capacity(), b.edges.capacity());
        assert_eq!(a.edges.iter().map(|e| (e.from, e.to, e.action)).collect::<Vec<_>>(),
            b.edges.iter().map(|e| (e.from, e.to, e.action)).collect::<Vec<_>>());
    }
    for (a, b) in a.actions.iter().zip(&b.actions) {
        assert_eq!(&*a.hits, &*b.hits);
        assert_eq!((a.factor.perfect, a.factor.inside, a.factor.slot, a.factor.fever,
            a.factor.rank, a.factor.reference.to_bits()),
            (b.factor.perfect, b.factor.inside, b.factor.slot, b.factor.fever,
            b.factor.rank, b.factor.reference.to_bits()));
    }
    assert_eq!(a.touches.len(), b.touches.len());
    for (a, b) in a.touches.iter().zip(&b.touches) {
        assert_eq!(a.layer, b.layer);
        assert_eq!(a.inputs, b.inputs);
    }
    assert_eq!(a.temporal.len(), b.temporal.len());
    for (a, b) in a.temporal.iter().zip(&b.temporal) {
        assert_eq!((a.start, a.after_first, a.resume, a.first_frame, a.last_frame, a.count),
            (b.start, b.after_first, b.resume, b.first_frame, b.last_frame, b.count));
        assert_eq!(a.permutation, b.permutation);
    }
}

#[test]
fn native_frame_templates_preserve_full_actions_dag_ids_temporal_proofs_and_budgets() {
    for correction in [None, Some(JudgementCorrection::Great), Some(JudgementCorrection::Bad)] {
        for constrained in [false, true] {
            let mut request = input(&[0.2, 1.3, 2.5, 3.7, 4.9, 6.1,
                0.55, 0.558, 0.575, 0.59, 2.9, 2.91, 6.45], vec![ReplaySkill {
                    duration: 0.1, correction,
                    score: crate::hit_timing::ScoreEffect::Plain { percent: 25. },
                }; 6], 1);
            request.native_time_routing = None;
            request.fever_frames = Some([70, 650]);
            if constrained {
                request.constraints.push(crate::hit_timing::NoteConstraint { before: 6, after: 8,
                    minimum_gap_frames: 1, require_dispatch_order: true, earliest_after: None });
            }
            let prepared = Prepared::new(&request).unwrap();
            let row = prepared.joint_row();
            let mut a = WindowStats::default();
            let mut b = WindowStats::default();
            let opts = options();
            let old = prepared.compile(&row, false, WindowOptions { frame_templates: false, ..opts }, &mut a).unwrap();
            let new = prepared.compile(&row, false, WindowOptions { frame_templates: true, ..opts }, &mut b).unwrap();
            assert_graph(&old, &new);
            assert_eq!(serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
            for budget in [0, 32, 200, old.edge_count, old.edge_count.saturating_sub(1)] {
                let query = |enabled| {
                    let mut stats = WindowStats::default();
                    let graph = prepared.compile(&row, false, WindowOptions {
                        frame_templates: enabled, max_graph_edges: budget, ..opts }, &mut stats);
                    (graph, stats)
                };
                let (a, a_stats) = query(false);
                let (b, b_stats) = query(true);
                assert_eq!(a.is_some(), b.is_some());
                assert_eq!(serde_json::to_value(a_stats).unwrap(), serde_json::to_value(b_stats).unwrap());
                if let (Some(a), Some(b)) = (a, b) { assert_graph(&a, &b); }
            }
        }
    }
}
