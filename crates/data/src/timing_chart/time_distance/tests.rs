use super::*;
use serde_json::json;
use bangdream_optimize_core::hit_timing::time_judgement;

fn scoring(skills: usize) -> TimeDistanceScoring {
    TimeDistanceScoring {
        power: 300000.25,
        play_level: 27,
        initial_combo: 0,
        medley_combo: true,
        fever: false,
        skills: (0..skills)
            .map(|_| ReplaySkill {
                duration: 5.,
                score: bangdream_optimize_core::hit_timing::ScoreEffect::Plain { percent: 100. },
                correction: None,
            })
            .collect(),
    }
}

#[test]
fn raw_chart_types_reach_distinct_windows_and_skill_markers_do_not_change_them() {
    for skill in [false,true] {
        let raw=json!([
            {"type":"BPM","beat":0,"bpm":120},
            {"type":"Single","beat":2,"lane":0,"skill":skill},
            {"type":"Single","beat":4,"lane":0,"flick":true},
            {"type":"Directional","beat":6,"lane":0,"direction":"Left"},
            {"type":"Long","connections":[{"beat":8,"lane":0},{"beat":10,"lane":0,"skill":skill}]},
            {"type":"Long","connections":[{"beat":12,"lane":0},{"beat":14,"lane":0,"flick":true}]},
            {"type":"Slide","connections":[{"beat":16,"lane":0,"skill":skill},
                {"beat":18,"lane":1,"skill":skill},{"beat":20,"lane":2,"skill":skill}]},
            {"type":"Slide","connections":[{"beat":22,"lane":0},{"beat":24,"lane":1,"flick":true,"skill":skill}]},
            {"type":"Slide","connections":[{"beat":26,"lane":0},{"beat":28,"lane":1,"direction":"Right"}]}
        ]);
        let chart=TimeDistanceChart::from_bestdori(&raw).unwrap();
        let slide=|role| NoteWindow::SlideTime{role,last_frame:None};
        assert_eq!(chart.notes.iter().map(|n|match n.window { NoteWindow::SlideTime {role,..} => slide(role), w=>w }).collect::<Vec<_>>(),vec![
            NoteWindow::Standard,NoteWindow::Standard,NoteWindow::Standard,
            NoteWindow::Standard,NoteWindow::WidenedEndpoint,
            NoteWindow::Standard,NoteWindow::WidenedEndpoint,
            slide(SlideTimeRole::Head),slide(SlideTimeRole::Middle),slide(SlideTimeRole::End),
            slide(SlideTimeRole::Head),slide(SlideTimeRole::FlickEnd),
            slide(SlideTimeRole::Head),slide(SlideTimeRole::DirectionalEnd)]);
        let input=chart.input(scoring(chart.skill_count())).unwrap();
        let replayed=replay(&input).unwrap();
        assert!(replayed.trace.iter().all(|t|t.raw==Judgement::Perfect));
        assert_eq!(replayed.routing_coverage.time_candidate_notes,14);
        for (i, first) in [(4,593),(7,954),(8,1078),
            (9,1194),(11,1440),(13,1680)] {
            let domain=chart.notes[i].frame_domain(Judgement::Perfect).unwrap();
            assert_eq!(domain[0],first,"note {i}");
        }
        // Actual f32 progress at t=8 causes timeout one frame before the
        // nominal t+M bound. Ordinary flick instead uses its 14-update counter.
        assert_eq!(chart.notes[7].frame_domain(Judgement::Perfect).unwrap().last(),Some(&985));
        assert_eq!(chart.notes[11].frame_domain(Judgement::Perfect).unwrap().last(),Some(&1454));
    }
}

#[test]
fn fractional_flick_tail_baseline_and_restricted_dp_respect_arrival_gate() {
    use bangdream_optimize_core::hit_timing::independent::JointTimeline;
    let chart=TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":150},
        {"type":"Slide","connections":[{"beat":5,"lane":0},
            {"beat":7.+1./48.,"lane":1,"flick":true}]}
    ])).unwrap();
    let input=chart.input(scoring(0)).unwrap();
    let reference=input.chart_reference_frame(1).unwrap();
    let first=*input.notes[1].frame_domain(Judgement::Perfect).unwrap().first().unwrap();
    assert_eq!(first,reference);
    let limits=[None,Some([first-1,first-1])];
    assert!(JointTimeline::maximum_in_frame_ranges(&input,&limits).unwrap().is_none());
    let range=bangdream_optimize_core::hit_timing::ranges::TimingSongRanges {
        song_id:1, difficulty:3, reference:input.clone(),
        ranges:vec![bangdream_optimize_core::hit_timing::ranges::RelativeFrameRange {
            note_index:1, first_offset:0, last_offset:0 }],
    };
    let zero_limits=range.limits(&input).unwrap();
    assert_eq!(zero_limits[1],Some([first,first]));
    let winner=JointTimeline::maximum_in_frame_ranges(&input,&zero_limits).unwrap().unwrap();
    assert_eq!(winner.result.hit_frames[1],first);
}

#[test]
fn exact_tail_arrival_keeps_the_input_before_the_fourteenth_stop_update() {
    use bangdream_optimize_core::hit_timing::independent::JointTimeline;
    for bpm_change in [false, true] {
        let mut events = vec![json!({"type":"BPM","beat":0,"bpm":120}),
            json!({"type":"Slide","connections":[{"beat":2,"lane":0},
                {"beat":4,"lane":1,"flick":true}]})];
        if bpm_change { events.push(json!({"type":"BPM","beat":4.25,"bpm":480})); }
        let chart = TimeDistanceChart::from_bestdori(&json!(events)).unwrap();
        let mut input = chart.input(scoring(0)).unwrap();
        assert_eq!(input.chart_reference_frame(1).unwrap(), 240);
        assert_eq!(input.notes[1].window,
            NoteWindow::SlideTime {role:SlideTimeRole::FlickEnd, last_frame:Some(254)});
        assert_eq!(input.native_time_routing.as_ref().unwrap().targets[1].as_ref().unwrap()
            .active_until_frame, 254);
        for (frame, feasible) in [(254, true), (255, false)] {
            input.hit_frames = vec![120, frame];
            assert_eq!(replay(&input).is_ok(), feasible);
            // Keep a feasible baseline; query the boundary with explicit limits.
            input.hit_frames[1] = 240;
            let winner = JointTimeline::maximum_in_frame_ranges(&input,
                &[Some([120,120]), Some([frame,frame])]).unwrap();
            assert_eq!(winner.is_some(), feasible);
            if let Some(winner) = winner {
                input.hit_frames = winner.result.hit_frames.clone();
                input.hit_order = winner.result.hit_order.clone();
                assert_eq!(replay(&input).unwrap(), winner.result);
            }
        }
    }
}

#[test]
fn reference_frames_match_shared_native_and_browser_cases() {
    use bangdream_optimize_core::hit_timing::{NativeTimeRouting, NativeTimeTarget, ReplayNote};
    #[derive(serde::Deserialize)]
    struct Case { name:String, note:ReplayNote, target:Option<NativeTimeTarget>, expected:Option<u32> }
    let cases:Vec<Case>=serde_json::from_str(include_str!("../../../../core/tests/fixtures/chart-reference-frames.json")).unwrap();
    for case in cases {
        let input=ReplayInput {power:300000.,play_level:27,fps:REPLAY_FPS,
            notes:vec![case.note],hit_frames:vec![0],hit_order:vec![],skills:vec![],
            initial_combo:0,medley_combo:true,fever_frames:None,constraints:vec![],
            native_time_routing:Some(NativeTimeRouting {distance_mode:CandidateDistanceMode::ChartTime,
                music_positions:vec![],targets:vec![case.target],candidate_order:vec![0],
                connections:vec![],event_order:vec![]})};
        assert_eq!(input.chart_reference_frame(0).ok(),case.expected,"{}",case.name);
    }
}

#[test]
fn slide_timeout_uses_current_bpm_and_first_expiry_is_final() {
    // Tail at t=2, then a 4x speed-up at t=2.125: native distance/current-BPM
    // timeout moves beyond t+M. A nominal 216.667ms cap would lose valid P hits.
    let chart=TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":120},
        {"type":"Slide","connections":[{"beat":2,"lane":0},{"beat":4,"lane":1}]},
        {"type":"BPM","beat":4.25,"bpm":480}
    ])).unwrap();
    let input=chart.input(scoring(0)).unwrap();
    let end=*input.notes[1].frame_domain(Judgement::Perfect).unwrap().last().unwrap();
    assert!(end>266,"BPM-dependent lifetime must not be clipped: {end}");
    let mut expected_tempo=TempoMap::new(&json!([
        {"type":"BPM","beat":0,"bpm":120},{"type":"BPM","beat":4.25,"bpm":480}
    ]).as_array().unwrap()).unwrap();
    let clocks=expected_tempo.music_clocks(end+2).unwrap();
    let late=|frame:usize| ((240.0f32/clocks[frame].current_bpm)
        *(clocks[frame].adjusted_music_pos-192.0f32))/192.0f32;
    assert!(late(end as usize)<=f32::from_bits(0x3e5dddde));
    assert!(late(end as usize+1)>f32::from_bits(0x3e5dddde));
    let mut late_hit=input;
    late_hit.hit_frames[1]=end;
    assert_eq!(replay(&late_hit).unwrap().trace[1].raw,Judgement::Perfect);
    late_hit.hit_frames[1]=end+1;
    assert!(replay(&late_hit).is_err());
}

#[test]
fn unsupported_or_malformed_note_kinds_do_not_silently_use_ordinary_windows() {
    for tail in [json!({"beat":4,"lane":1,"flick":"true"}),
        json!({"beat":4,"lane":1,"direction":"Up"})] {
        assert!(TimeDistanceChart::from_bestdori(&json!([
            {"type":"BPM","beat":0,"bpm":120},
            {"type":"Slide","connections":[{"beat":2,"lane":0},tail]}
        ])).is_err());
    }
    for nodes in [json!([{"beat":2,"lane":0},{"beat":3,"lane":0},{"beat":4,"lane":0}]),
        json!([{"beat":2,"lane":0},{"beat":4,"lane":0,"direction":"Left"}])] {
        assert!(TimeDistanceChart::from_bestdori(&json!([
            {"type":"BPM","beat":0,"bpm":120},{"type":"Long","connections":nodes}
        ])).is_err());
    }
}
fn slide() -> Value {
    // BPM 150 advances by exactly one f32 tick per update.
    json!([
        {"type":"BPM","beat":0,"bpm":150},
        {"type":"Slide","connections":[
            {"beat":10,"lane":1},
            {"beat":10.25,"lane":2},
            {"beat":10.5,"lane":3}
        ]}
    ])
}

#[test]
fn midpoint_equality_survives_and_the_next_frame_expires() {
    let prepared = TimeDistanceChart::from_bestdori(&slide()).unwrap();
    assert_eq!(
        (
            prepared.head_midpoint_cutoffs,
            prepared.middle_midpoint_cutoffs
        ),
        (1, 1)
    );
    let mut input = prepared.input(scoring(0)).unwrap();
    let route = input.native_time_routing.as_ref().unwrap();
    assert_eq!(route.targets[0].as_ref().unwrap().active_until_frame, 486);
    assert_eq!(route.targets[1].as_ref().unwrap().active_until_frame, 498);
    assert_eq!(input.constraints.len(), 2);
    // Exhaust the near-midpoint combinations independently of the preparer's
    // stored targets. Both offsets still receive at least Great in this range.
    for head in 481..=489 {
        for middle in 493..=501 {
            input.hit_frames = vec![head, middle, 504];
            assert_eq!(
                replay(&input).is_ok(),
                head <= 486 && middle <= 498,
                "head={head}, middle={middle}"
            );
        }
    }
}

#[test]
fn dense_slide_expiry_waits_for_the_handler_and_observes_successor_stop() {
    use bangdream_optimize_core::hit_timing::independent::JointTimeline;
    // Exactly one tick per update. The middle at x=240 switches to Stop in
    // U_240, so its first Stop handler is U_241 (clock C_242). The successor
    // at x=241 switches in U_241, AFTER the middle's handler. Recall first
    // runs in U_242; I_242 is still valid. No sample satisfies its midpoint.
    let chart = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":150},
        {"type":"Slide","connections":[
            {"beat":239./48.,"lane":0}, {"beat":5,"lane":1},
            {"beat":241./48.,"lane":2}
        ]}
    ])).unwrap();
    let mut input = chart.input(scoring(0)).unwrap();
    let targets = &input.native_time_routing.as_ref().unwrap().targets;
    assert_eq!(targets[0].as_ref().unwrap().active_until_frame, 240);
    assert_eq!(targets[1].as_ref().unwrap().active_until_frame, 242);
    for (frame, valid) in [(240, true), (241, true), (242, true), (243, false)] {
        input.hit_frames = vec![239, frame, frame.max(241)];
        input.hit_order.clear();
        assert_eq!(replay(&input).is_ok(), valid, "frame {frame}");
        let limits = [Some([239,239]), Some([frame,frame]), Some([frame.max(241);2])];
        input.hit_frames = vec![239,240,241];
        assert_eq!(JointTimeline::maximum_in_frame_ranges(&input, &limits).unwrap().is_some(), valid);
    }
    // Head x=240: U_240 only switches to Wait; U_241 first tests C_242.
    let head = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":150},
        {"type":"Slide","connections":[{"beat":5,"lane":0},{"beat":241./48.,"lane":1}]}
    ])).unwrap();
    assert_eq!(head.routing.targets[0].as_ref().unwrap().active_until_frame, 241);
}

#[test]
fn bpm_drop_cannot_timeout_an_endpoint_during_move() {
    // At C_240, retained f32 progress is slightly past tick 192. A very small
    // new BPM magnifies that residual above M. U_239/U_240 still execute Move;
    // expiry is only evaluated by the first Stop handler U_241, after I_241.
    for tail in [json!({"beat":4,"lane":1}),
        json!({"beat":4,"lane":1,"direction":"Right"})] {
        let chart = TimeDistanceChart::from_bestdori(&json!([
            {"type":"BPM","beat":0,"bpm":120},
            {"type":"BPM","beat":4,"bpm":0.001},
            {"type":"Slide","connections":[{"beat":2,"lane":0},tail]}
        ])).unwrap();
        let mut input = chart.input(scoring(0)).unwrap();
        assert_eq!(input.notes[1].frame_domain(Judgement::Perfect).unwrap().last(), Some(&241));
        for (frame, valid) in [(240, true), (241, true), (242, false)] {
            input.hit_frames = vec![120, frame];
            assert_eq!(replay(&input).is_ok(), valid, "frame {frame}");
        }
    }
}

#[test]
fn prepared_lifetimes_match_a_forward_single_handler_state_walk() {
    // Independent composition oracle: no ceil/floor, cutoff search or product
    // lifetime helper. Music-clock arithmetic has separate clock tests; this
    // test checks its composition with input/update and source-node ordering.
    fn check(raw: Value) {
        let chart = TimeDistanceChart::from_bestdori(&raw).unwrap();
        let clocks = TempoMap::new(raw.as_array().unwrap()).unwrap()
            .music_clocks(600 * REPLAY_FPS).unwrap();
        let n = chart.notes.len();
        let mut stopped = vec![false; n];
        let mut counter = vec![0.0f32; n];
        let mut raw_last = vec![None; n];
        let mut live_last = vec![None; n];
        for frame in 0..600 * REPLAY_FPS {
            let clock = clocks[frame as usize + 1];
            for i in 0..n {
                if !stopped[i] {
                    if f64::from(frame + 1) > chart.notes[i].time * 120. {
                        stopped[i] = true;
                    }
                    continue;
                }
                let NoteWindow::SlideTime {role, ..} = chart.notes[i].window else { unreachable!() };
                let current = (chart.identities[i].beat * 48.).round() as i32 as f32;
                let since = clock.adjusted_music_pos - current;
                let late = ((240.0f32 / clock.current_bpm) * since) / 192.0f32;
                let timeout = if role == SlideTimeRole::FlickEnd {
                    counter[i] += 0.5;
                    counter[i] >= 7.
                } else { late > f32::from_bits(0x3e5dddde) };
                if timeout && raw_last[i].is_none() { raw_last[i] = Some(frame); }
                let expired = match role {
                    SlideTimeRole::Head | SlideTimeRole::Middle => {
                        let next = (chart.identities[i+1].beat * 48.).round() as i32 as f32;
                        let until = next - clock.adjusted_music_pos;
                        timeout || if role == SlideTimeRole::Head {
                            until <= 0. || since > until
                        } else {
                            stopped[i+1] || (until > 0. && since > until)
                        }
                    }
                    _ => timeout,
                };
                if expired && live_last[i].is_none() { live_last[i] = Some(frame); }
            }
            if raw_last.iter().all(Option::is_some) && live_last.iter().all(Option::is_some) { break; }
        }
        for i in 0..n {
            let domain = chart.notes[i].frame_domain(Judgement::Bad).unwrap();
            assert_eq!(*domain.last().unwrap(), raw_last[i].unwrap_or(72000), "raw {i}: {raw}");
            assert_eq!(chart.routing.targets[i].as_ref().unwrap().active_until_frame,
                live_last[i].unwrap_or(72000), "live {i}: {raw}");
        }
        let mut reference = chart.input(scoring(0)).unwrap();
        reference.hit_frames = reference.chart_reference_frames().unwrap();
        replay(&reference).unwrap();
    }
    for bpm in [120.,150.,185.,1500.] {
        for start_tick in [0,239,240,10000] {
            for gap in [1,3,12] {
                for kind in 0..3 {
                    let mut end = json!({"beat":f64::from(start_tick+2*gap)/48.,"lane":2});
                    if kind == 1 {end["direction"] = json!("Right");}
                    if kind == 2 {end["flick"] = json!(true);}
                    check(json!([
                        {"type":"BPM","beat":0,"bpm":bpm},
                        {"type":"Slide","connections":[
                            {"beat":f64::from(start_tick)/48.,"lane":0},
                            {"beat":f64::from(start_tick+gap)/48.,"lane":1},end]}
                    ]));
                }
            }
        }
    }
    // Rate changes can jump, retract and re-cross the timeout predicate.
    for rates in [[150.,30.,600.],[120.,480.,30.],[185.,1500.,120.]] {
        check(json!([
            {"type":"BPM","beat":0,"bpm":rates[0]},
            {"type":"BPM","beat":5,"bpm":rates[1]},
            {"type":"BPM","beat":5.25,"bpm":rates[2]},
            {"type":"Slide","connections":[{"beat":4.75,"lane":0},
                {"beat":5,"lane":1},{"beat":5.5,"lane":2}]}
        ]));
    }
    check(json!([
        {"type":"BPM","beat":0,"bpm":120},
        {"type":"Slide","connections":[{"beat":1199,"lane":0},{"beat":1200,"lane":1}]}
    ]));
}

#[test]
fn hidden_nodes_do_not_supply_positions_callbacks_or_deadlines() {
    let mut raw = json!([
        {"type":"BPM","beat":0,"bpm":150},
        {"type":"Slide","connections":[
            {"beat":10,"lane":1},
            {"beat":10.1,"lane":1.5,"hidden":true},
            {"beat":10.25,"lane":2},
            {"beat":10.5,"lane":3}
        ]}
    ]);
    let before = TimeDistanceChart::from_bestdori(&raw).unwrap();
    raw[1]["connections"][1]["beat"] = json!(-1000);
    raw[1]["connections"][1]["lane"] = json!("unused");
    let after = TimeDistanceChart::from_bestdori(&raw).unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
}

#[test]
fn same_lane_ties_use_source_order_and_settled_notes_leave_candidates() {
    let chart = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":120},
        {"type":"Single","beat":2,"lane":1},
        {"type":"Single","beat":2,"lane":1}
    ]))
    .unwrap();
    let mut input = chart.input(scoring(0)).unwrap();
    assert_eq!(input.hit_frames, [120, 120]);
    assert_eq!(replay(&input).unwrap().trace.len(), 2);
    input.hit_order = vec![1, 0];
    assert!(matches!(
        replay(&input),
        Err(ReplayError::InputTarget {
            index: 1,
            selected: Some(0),
            ..
        })
    ));
}

#[test]
fn dense_long_endpoints_allow_same_frame_but_preserve_dispatch_order() {
    let chart = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":300},
        {"type":"Long","connections":[
            {"beat":10.01,"lane":0}, {"beat":10.02,"lane":1}
        ]}
    ]))
    .unwrap();
    let mut input = chart.input(scoring(0)).unwrap();
    assert!(input.constraints.iter().all(|e| e.minimum_gap_frames == 0 && e.require_dispatch_order));
    assert!(input.hit_frames.windows(2).any(|f| f[1] == f[0]));
    replay(&input).unwrap();
    input.hit_order = vec![1, 0];
    assert!(matches!(
        replay(&input),
        Err(ReplayError::Constraint { .. })
    ));
}

#[test]
fn dense_slide_reference_plan_survives_ranges_that_straddle_zero() {
    use bangdream_optimize_core::hit_timing::independent::JointTimeline;
    let chart = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":1500},
        {"type":"Slide","connections":(0..4).map(|i|json!({
            "beat":50.+f64::from(i+1)/48.,"lane":i
        })).collect::<Vec<_>>()}
    ])).unwrap();
    let baseline = chart.input(scoring(0)).unwrap();
    let mut reference = baseline.clone();
    reference.hit_frames = reference.chart_reference_frames().unwrap();
    assert_eq!(reference.hit_frames, [240;4]);
    assert!(replay(&reference).unwrap().trace.iter().all(|t|t.raw==Judgement::Perfect));
    let fixed_limits:Vec<_>=reference.hit_frames.iter().map(|&f|Some([f,f])).collect();
    assert!(JointTimeline::maximum_in_frame_ranges(&baseline,&fixed_limits).unwrap().is_some());
    let limits=vec![Some([239,241]);4];
    let winner=JointTimeline::maximum_in_frame_ranges(&baseline,&limits).unwrap().unwrap();
    let mut checked=baseline.clone();
    checked.hit_frames=winner.result.hit_frames.clone();
    checked.hit_order=winner.result.hit_order.clone();
    assert_eq!(replay(&checked).unwrap(),winner.result);
    // The former requirement removes every assignment even though all four
    // individual reference frames are Perfect and precede their deadlines.
    let mut old=baseline;
    for edge in &mut old.constraints {edge.minimum_gap_frames=1;}
    // Middle nodes can only start two input frames before nominal arrival;
    // the old one-frame gap need not even have an unrestricted baseline here.
    old.hit_frames=vec![238,239,240,241];
    replay(&old).unwrap();
    assert!(JointTimeline::maximum_in_frame_ranges(&old,&limits).unwrap().is_none());
    reference.hit_order=vec![1,0,2,3];
    assert!(matches!(replay(&reference),Err(ReplayError::Constraint{..})));
}

#[test]
fn raw_bad_window_endpoints_are_inclusive_candidate_frames() {
    let chart = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":120},
        {"type":"Single","beat":4,"lane":0}
    ]))
    .unwrap();
    let target = chart.routing.targets[0].as_ref().unwrap();
    assert_eq!(
        (target.active_from_frame, target.active_until_frame),
        (225, 255)
    );
    for (frame, expected) in [
        (224, Judgement::Miss),
        (225, Judgement::Bad),
        (255, Judgement::Bad),
        (256, Judgement::Miss),
    ] {
        assert_eq!(
            time_judgement(f64::from(frame) / 120. - 2., false).unwrap(),
            expected
        );
    }
}

#[test]
fn source_identity_skill_order_and_fever_survive_preparation() {
    let chart = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":120},
        {"type":"Single","beat":6,"lane":2,"skill":true},
        {"type":"Single","beat":2,"lane":1,"skill":true},
        {"type":"System","beat":2,"data":"cmd_fever_start.wav"},
        {"type":"System","beat":6,"data":"cmd_fever_end.wav"}
    ]))
    .unwrap();
    assert_eq!(
        chart
            .identities
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["2:0", "1:0"]
    );
    let mut context = scoring(2);
    context.fever = true;
    let input = chart.input(context).unwrap();
    assert_eq!(input.fever_frames, Some([120, 360]));
    assert_eq!(
        input.notes.iter().map(|n| n.trigger).collect::<Vec<_>>(),
        [Some(0), Some(1)]
    );
    assert_eq!(
        replay(&input)
            .unwrap()
            .routing_coverage
            .time_candidate_notes,
        2
    );
}

#[test]
fn rejects_invalid_visible_input_explicitly() {
    for field in ["lane", "beat"] {
        let mut raw = slide();
        raw[1]["connections"][0][field] = json!(-1);
        assert!(TimeDistanceChart::from_bestdori(&raw).is_err());
    }
    let mut raw = slide();
    raw[1]["connections"][1]["beat"] = json!(10.251);
    assert!(TimeDistanceChart::from_bestdori(&raw).is_err());
    let mut raw = slide();
    raw[1]["type"] = json!("Unknown");
    assert!(TimeDistanceChart::from_bestdori(&raw).is_err());
}

#[test]
fn variable_bpm_times_skills_and_fever_follow_sorted_segments() {
    let chart = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":8,"bpm":60},
        {"type":"Single","beat":10,"lane":2,"skill":true},
        {"type":"BPM","beat":0,"bpm":120},
        {"type":"Single","beat":3,"lane":0,"skill":true},
        {"type":"BPM","beat":4,"bpm":240},
        {"type":"Single","beat":4,"lane":1},
        {"type":"Single","beat":8,"lane":2,"skill":true},
        {"type":"System","beat":4,"data":"cmd_fever_start.wav"},
        {"type":"System","beat":10,"data":"cmd_fever_end.wav"}
    ]))
    .unwrap();
    assert_eq!(
        chart.notes.iter().map(|n| n.time).collect::<Vec<_>>(),
        [1.5, 2., 3., 5.]
    );
    assert_eq!(
        chart.notes.iter().map(|n| n.trigger).collect::<Vec<_>>(),
        [Some(0), None, Some(1), Some(2)]
    );
    assert_eq!(
        chart
            .identities
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["3:0", "5:0", "6:0", "1:0"]
    );
    let mut context = scoring(3);
    context.fever = true;
    let input = chart.input(context).unwrap();
    assert_eq!(input.fever_frames, Some([240, 600]));
    for (note, target) in chart.notes.iter().zip(&chart.routing.targets) {
        let domain = note.frame_domain(Judgement::Bad).unwrap();
        let target = target.as_ref().unwrap();
        assert_eq!(target.active_from_frame, domain[0]);
        assert_eq!(target.active_until_frame, *domain.last().unwrap());
    }
}

#[test]
fn clock_rate_changes_after_boundary_update_without_resetting() {
    for (bpm, step) in [(300, 2.), (75, 0.5)] {
        let raw = json!([
            {"type":"BPM","beat":0,"bpm":150},
            {"type":"BPM","beat":4,"bpm":bpm}
        ]);
        let mut tempo = TempoMap::new(raw.as_array().unwrap()).unwrap();
        let positions = tempo.music_positions(194).unwrap();
        assert_eq!(tempo.0[1].applied_frame, Some(192));
        assert_eq!(
            &positions[191..],
            &[191., 192., 192. + step, 192. + 2. * step]
        );
    }
    let raw = json!([
        {"type":"BPM","beat":0,"bpm":300},
        {"type":"BPM","beat":193. / 48.,"bpm":75}
    ]);
    let mut tempo = TempoMap::new(raw.as_array().unwrap()).unwrap();
    let positions = tempo.music_positions(99).unwrap();
    assert_eq!(tempo.0[1].applied_frame, Some(97));
    // Crossing between frames retains the one-tick overshoot in the new bar.
    assert_eq!(&positions[96..], &[192., 194., 194.5, 195.]);
}

#[test]
fn dense_bpm_commands_preserve_removal_order() {
    let raw = json!([
        {"type":"BPM","beat":0,"bpm":600},
        {"type":"BPM","beat":1. / 48.,"bpm":300},
        {"type":"BPM","beat":2. / 48.,"bpm":150},
        {"type":"BPM","beat":3. / 48.,"bpm":75}
    ]);
    let mut tempo = TempoMap::new(raw.as_array().unwrap()).unwrap();
    let positions = tempo.music_positions(3).unwrap();
    assert_eq!(positions, [0., 4., 4.5, 5.5]);
    assert_eq!(
        tempo.0.iter().map(|s| s.applied_frame).collect::<Vec<_>>(),
        [Some(0), Some(1), Some(2), Some(1)]
    );
}

#[test]
fn redundant_bpm_changes_preserve_long_running_f32_drift() {
    let raw = json!([
        {"type":"BPM","beat":0,"bpm":185},
        {"type":"BPM","beat":4,"bpm":185},
        {"type":"BPM","beat":400.25,"bpm":185},
        {"type":"BPM","beat":1000,"bpm":185}
    ]);
    let events = raw.as_array().unwrap();
    let fixed = TempoMap::new(&events[..1])
        .unwrap()
        .music_positions(72000)
        .unwrap();
    let changed = TempoMap::new(events)
        .unwrap()
        .music_positions(72000)
        .unwrap();
    assert_eq!(fixed, changed);
}

#[test]
fn slide_crossing_bpm_change_uses_continuous_clock_midpoints() {
    let chart = TimeDistanceChart::from_bestdori(&json!([
        {"type":"BPM","beat":0,"bpm":150},
        {"type":"BPM","beat":4,"bpm":300},
        {"type":"Slide","connections":[
            {"beat":3.875,"lane":0}, {"beat":4.125,"lane":1}, {"beat":4.375,"lane":2}
        ]}
    ]))
    .unwrap();
    assert_eq!(
        chart.notes.iter().map(|n| n.time).collect::<Vec<_>>(),
        [1.55, 1.625, 1.675]
    );
    let mut input = chart.input(scoring(0)).unwrap();
    assert_eq!(input.hit_frames, [186, 195, 201]);
    let targets = &chart.routing.targets;
    assert_eq!(targets[0].as_ref().unwrap().active_until_frame, 192);
    assert_eq!(targets[1].as_ref().unwrap().active_until_frame, 198);
    for head in 190..=195 {
        for middle in 196..=200 {
            input.hit_frames = vec![head, middle, 201];
            let result = replay(&input);
            assert_eq!(
                result.is_ok(),
                head <= 192 && middle <= 198,
                "head={head}, middle={middle}, result={result:?}"
            );
        }
    }
}

#[test]
fn bpm_change_after_last_bad_window_does_not_change_existing_notes() {
    let mut raw = slide();
    let before = TimeDistanceChart::from_bestdori(&raw).unwrap();
    raw.as_array_mut()
        .unwrap()
        .push(json!({"type":"BPM","beat":100,"bpm":120}));
    let after = TimeDistanceChart::from_bestdori(&raw).unwrap();
    assert_eq!(after.bpm_segments[1].applied_frame, Some(4800));
    assert_eq!(
        serde_json::to_value(before.input(scoring(0)).unwrap()).unwrap(),
        serde_json::to_value(after.input(scoring(0)).unwrap()).unwrap()
    );
}

#[test]
fn invalid_tempo_maps_fail_explicitly() {
    for bpm in [0., -1., 1e300, 1e-300] {
        let raw = json!([{ "type":"BPM","beat":0,"bpm":bpm }]);
        assert!(TempoMap::new(raw.as_array().unwrap()).is_err());
    }
    for rows in [
        json!([]),
        json!([{"type":"BPM","beat":1,"bpm":120}]),
        json!([{"type":"BPM","beat":-1,"bpm":120}]),
        json!([{"type":"BPM","beat":0,"bpm":120},{"type":"BPM","beat":0,"bpm":240}]),
        json!([{"type":"BPM","beat":0,"bpm":120},{"type":"BPM","beat":1.001,"bpm":240}]),
        json!([{"type":"BPM","beat":0,"bpm":120},{"type":"BPM","beat":1201,"bpm":240}]),
    ] {
        assert!(TempoMap::new(rows.as_array().unwrap()).is_err(), "{rows}");
    }
}
