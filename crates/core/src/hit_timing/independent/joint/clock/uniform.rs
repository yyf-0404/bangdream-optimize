//! A closed interval in which advancing an immutable FIFO clock changes
//! neither the queue, its ready time nor the active head. Native start equality
//! is inactive; end equality is active. No position or speed model is used.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Proof {
    pub(super) first: u32,
    pub(super) last: u32,
    pub(super) active: Option<usize>,
}

pub(super) fn prove(ready:u32,windows:&[Window],queued:bool) -> Option<Proof> {
    let ready_last=if ready==0 {u32::MAX} else {ready-1};
    if !queued {
        return match windows {
            []=>Some(Proof {first:0,last:ready_last,active:None}),
            [window] if window.start==0=>Some(Proof {first:1,
                last:window.end.min(ready_last),active:Some(usize::from(window.slot))}),
            _=>None,
        };
    }
    let active=windows.first().filter(|window|window.start==0)
        .map(|window|usize::from(window.slot));
    let first=u32::from(active.is_some());
    let last=windows.iter().fold(ready_last,|last,window| {
        let last=last.min(window.end);
        if window.start==0 {last} else {last.min(window.start)}
    });
    (first<=last).then_some(Proof {first,last,active})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proved_queue_intervals_preserve_all_clock_fields_active_head_and_progress() {
        let options=[Window {start:0,end:4,slot:0},Window {start:2,end:5,slot:1},
            Window {start:5,end:8,slot:2},Window {start:0,end:0,slot:3}];
        for ready in [0,1,3,9,u32::MAX] {
            for length in 0..=3usize {
                for pattern in 0..options.len().pow(length as u32) {
                    let windows:Vec<_>=(0..length)
                        .map(|i|options[pattern/options.len().pow(i as u32)%options.len()]).collect();
                    let Some(proof)=prove(ready,&windows,true) else {continue};
                    for frame in proof.first..=proof.last.min(12) {
                        let mut state=State {ready,windows:Rc::new(windows.clone()),progress:7};
                        let original=state.clone();
                        state.advance(frame);state.normalize_active_starts(frame);
                        assert!(state==original,"frame={frame}, ready={ready}, windows={windows:?}");
                        let active=state.windows.first().filter(|w|w.start<frame)
                            .map(|w|usize::from(w.slot));
                        assert_eq!(active,proof.active);
                    }
                }
            }
        }
    }

    #[test]
    fn queued_proof_has_the_original_inclusive_boundaries_and_no_overflow() {
        assert_eq!(prove(0,&[],true),Some(Proof {first:0,last:u32::MAX,active:None}));
        let head=Window {start:0,end:10,slot:0};
        let next=Window {start:6,end:20,slot:1};
        assert_eq!(prove(0,&[head,next],true),Some(Proof {first:1,last:6,active:Some(0)}));
        assert_eq!(prove(4,&[head,next],true),Some(Proof {first:1,last:3,active:Some(0)}));
        assert_eq!(prove(0,&[next],true),Some(Proof {first:0,last:6,active:None}));
        assert!(prove(0,&[head,next],false).is_none());
        assert_eq!(prove(0,&[head],true),prove(0,&[head],false));
        assert!(prove(1,&[head],true).is_none());
        let far=Window {start:u32::MAX,end:u32::MAX,slot:2};
        assert_eq!(prove(0,&[far],true),Some(Proof {first:0,last:u32::MAX,active:None}));
    }
}
