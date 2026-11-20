//! All clock changes occur at ready, start + 1, or end + 1. Between two
//! consecutive boundaries, FIFO expiry/reset, normalization and active head
//! are identical, including cells in which the original clock has expired.
use super::*;

pub(super) fn boundaries(ready:u32,windows:&[Window]) -> Vec<u32> {
    let mut result=Vec::with_capacity(1+2*windows.len());
    if ready!=0 {result.push(ready);}
    for window in windows {
        if let Some(boundary)=window.start.checked_add(1) {result.push(boundary);}
        if let Some(boundary)=window.end.checked_add(1) {result.push(boundary);}
    }
    result.sort_unstable();result.dedup();result
}

pub(super) fn cell(boundaries:&[u32],frame:u32) -> (u32,u32) {
    let next=boundaries.partition_point(|&boundary|boundary<=frame);
    let first=if next==0 {0} else {boundaries[next-1]};
    let last=boundaries.get(next).map_or(u32::MAX,|boundary|boundary-1);
    (first,last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cell_has_identical_complete_advanced_clock_reset_and_active_head() {
        let options=[Window {start:0,end:4,slot:0},Window {start:2,end:5,slot:1},
            Window {start:5,end:8,slot:2},Window {start:0,end:0,slot:3}];
        for ready in [0,1,3,9,u32::MAX] {
            for length in 0..=3usize {
                for pattern in 0..options.len().pow(length as u32) {
                    let windows:Vec<_>=(0..length).map(|i|
                        options[pattern/options.len().pow(i as u32)%options.len()]).collect();
                    let boundaries=boundaries(ready,&windows);
                    for normalize in [false,true] {
                        for frame in 0..=12 {
                            let (first,last)=cell(&boundaries,frame);
                            let advance=|at| {
                                let mut state=State {ready,windows:Rc::new(windows.clone()),progress:7};
                                state.advance(at);
                                if normalize {state.normalize_active_starts(at);}
                                let active=state.windows.first().filter(|w|w.start<at).map(|w|w.slot);
                                (state,active)
                            };
                            assert_eq!(advance(frame),advance(first));
                            assert_eq!(advance(frame),advance(last.min(12)));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn maximum_frame_and_inclusive_start_end_ready_boundaries_do_not_overflow() {
        let windows=[Window {start:2,end:5,slot:0}];
        let points=boundaries(8,&windows);
        assert_eq!(points,[3,6,8]);
        assert_eq!(cell(&points,2),(0,2));assert_eq!(cell(&points,5),(3,5));
        assert_eq!(cell(&points,6),(6,7));assert_eq!(cell(&points,8),(8,u32::MAX));
        let far=[Window {start:u32::MAX,end:u32::MAX,slot:0}];
        assert_eq!(cell(&boundaries(u32::MAX,&far),u32::MAX),(u32::MAX,u32::MAX));
        assert_eq!(cell(&boundaries(0,&far),u32::MAX),(0,u32::MAX));
    }
}
