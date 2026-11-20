//! Clock endpoints are observed only by hit actions. Flooring an endpoint to
//! the nearest possible hit preserves `endpoint < hit` at EVERY graph action.
//! The exact ready frame is retained whenever a future trigger could observe
//! it; it is never rounded and never derived from a floored playing endpoint.
use super::*;

pub(super) struct Buckets {
    frames: Vec<u32>,
    next_trigger: Vec<u32>,
}
impl Buckets {
    pub(super) fn new(input: &ReplayInput, graph: &Graph) -> Self {
        let mut frames=vec![];
        let mut next_trigger=vec![u32::MAX;input.skills.len()+1];
        for action in &graph.actions {
            for &(note,frame) in &action.hits {
                frames.push(frame);
                if let Some(slot)=input.notes[note].trigger {
                    next_trigger[slot]=next_trigger[slot].min(frame);
                }
            }
        }
        frames.sort_unstable();frames.dedup();
        for slot in (0..input.skills.len()).rev() {
            next_trigger[slot]=next_trigger[slot].min(next_trigger[slot+1]);
        }
        Self {frames,next_trigger}
    }
    fn floor(&self, endpoint:u32)->u32 {
        let end=self.frames.partition_point(|&f| f<=endpoint);
        if end==0 {0} else {self.frames[end-1]}
    }
    pub(super) fn window(&self, slot:usize, start:u32, end:u32, ready:u32)->(Window,u32) {
        let window=Window {start:self.floor(start),end:self.floor(end),slot:slot as u8};
        // Geometry enforces strictly increasing trigger-slot dispatch order.
        // All later requests are >= this suffix minimum, so max(hit,ready)=hit.
        (window,if ready<=self.next_trigger[slot+1] {0} else {ready})
    }
    pub(super) fn bytes(&self)->usize {
        (self.frames.capacity()+self.next_trigger.capacity())*std::mem::size_of::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flooring_preserves_strict_activation_and_expiry_at_every_observable_frame() {
        let buckets=Buckets {frames:vec![0,1,7,30,31,99,201,900],next_trigger:vec![]};
        for endpoint in 0..1000 { for &frame in &buckets.frames {
            assert_eq!(endpoint<frame,buckets.floor(endpoint)<frame);
        } }
        let empty_prefix=Buckets {frames:vec![7,30,31],next_trigger:vec![]};
        for endpoint in 0..100 { for &frame in &empty_prefix.frames {
            assert_eq!(endpoint<frame,empty_prefix.floor(endpoint)<frame);
        } }
    }
    #[test]
    fn queued_start_uses_exact_ready_and_only_unobservable_cooling_is_erased() {
        let buckets=Buckets {frames:vec![1,7,30,99,201],next_trigger:vec![1,99,u32::MAX]};
        for ready in 0..300 {
            let (_,retained)=buckets.window(0,7,30,ready);
            for future in [99,100,120,201] {
                assert_eq!(future.max(ready),future.max(retained));
            }
        }
        assert_eq!(buckets.window(0,7,30,99).1,0);
        assert_eq!(buckets.window(0,7,30,100).1,100);
    }
}
