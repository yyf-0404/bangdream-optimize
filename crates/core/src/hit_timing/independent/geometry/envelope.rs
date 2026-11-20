//! Fixed outside domains determine the same optimistic ranks for every local
//! NET window. Rebuild choices only for intersecting notes and its trigger.
use super::*;
use std::sync::Arc;

pub(super) struct Cache {
    spans: Vec<(u32,u32,usize)>,
    last: Vec<u32>,
    bases: [Option<Arc<Basis>>;4],
    indexed: bool,
}
struct Basis { starts: Vec<u32>, ordered: Vec<u32>, missing: usize }
impl Cache {
    pub(super) fn new(intervals: &[Vec<Interval>]) -> Self {
        let mut spans:Vec<_>=intervals.iter().enumerate().filter_map(|(i,fs)|
            Some((fs.first()?.first,fs.last()?.last,i))).collect();
        spans.sort_unstable();
        let mut last=0;
        let ends=spans.iter().map(|s| { last=last.max(s.1);last }).collect();
        Self { spans,last:ends,bases:std::array::from_fn(|_| None),
            indexed:std::env::var("BANDORI_TIMING_DISABLE_NET_RANK_INDEX").as_deref()!=Ok("1") }
    }
    fn candidates(&self, first: u32, last: u32, trigger: usize) -> Vec<usize> {
        let end=self.spans.partition_point(|s| s.0<=last);
        let begin=self.last[..end].partition_point(|&f| f<first);
        let mut ids:Vec<_>=self.spans[begin..end].iter().filter(|s| s.1>=first).map(|s| s.2).collect();
        ids.push(trigger);ids.sort_unstable();ids.dedup();ids
    }
    pub(super) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()+self.spans.capacity()*std::mem::size_of::<(u32,u32,usize)>()
            +self.last.capacity()*std::mem::size_of::<u32>()
            +self.bases.iter().flatten().map(|b| std::mem::size_of::<Basis>()
                +(b.starts.capacity()+b.ordered.capacity())*std::mem::size_of::<u32>()).sum::<usize>()
    }
}
impl Prepared {
    pub(super) fn sparse_net_envelope(&self, trigger: usize, request: u32,
        playing: u32, correction: u32, rules: RowRules<'_>) -> Option<Vec<Vec<Factor>>> {
        let (basis,candidates,indexed)={
            let mut cache=self.envelope_cache.lock().unwrap();
            let key=rules.outside_correction.min(3) as usize;
            if cache.bases[key].is_none() {
                let mut choices=vec![];
                let starts:Vec<_>=(0..self.intervals.len()).map(|i| {
                    self.row_choices(i,usize::MAX,0,0,0,rules.outside_correction,false,&mut choices);
                    choices.first().map_or(u32::MAX,|c| c.first)
                }).collect();
                let missing=starts.iter().filter(|&&f| f==u32::MAX).count();
                let mut ordered=starts.clone();ordered.sort_unstable();
                cache.bases[key]=Some(Arc::new(Basis { starts,ordered,missing }));
            }
            (Arc::clone(cache.bases[key].as_ref().unwrap()),
                cache.candidates(request+1,request+playing,trigger),cache.indexed)
        };
        let mut starts=if indexed {vec![]} else {basis.starts.clone()};
        let mut missing=basis.missing;
        let mut removed=vec![];
        let mut added=vec![];
        let mut choices=vec![];
        let mut envelope=vec![];
        for i in candidates {
            self.row_choices(i,trigger,request,playing,correction,
                rules.outside_correction,rules.trigger_before,&mut choices);
            let first=choices.first()?.first;
            if indexed {
                let old=basis.starts[i];
                if old==u32::MAX {missing-=1;}
                if old!=first {removed.push(old);added.push(first);}
            } else {starts[i]=first;}
            let inside:Vec<_>=choices.iter().filter(|c| c.inside).map(|c| Factor {
                perfect:c.perfect,inside:true,slot:c.slot,fever:c.fever,
                reference:rules.references.map_or(0.,|r| r[i]),rank:c.last as usize,
            }).collect();
            if !inside.is_empty() { envelope.push(inside); }
        }
        if indexed {
            if missing!=0 {return None;}
            removed.sort_unstable();added.sort_unstable();
        } else {
            if starts.contains(&u32::MAX) { return None; }
            starts.sort_unstable();
        }
        for factor in envelope.iter_mut().flatten() {
            let frame=factor.rank as u32;
            factor.rank=if indexed {
                // Fixed outside counts plus exactly the replaced earliest
                // domains. Ties stay inclusive, as in the full sorted array.
                basis.ordered.partition_point(|&f| f<=frame)
                    +added.partition_point(|&f| f<=frame)
                    -removed.partition_point(|&f| f<=frame)
            } else {starts.partition_point(|&first| first<=frame)};
        }
        Some(envelope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_timing::{ReplayNote,ReplaySkill,ScoreEffect,NoteWindow};
    fn bits(value: Option<Vec<Vec<Factor>>>) -> Option<Vec<Vec<(bool,bool,u8,bool,u64,usize)>>> {
        value.map(|rows| rows.into_iter().map(|r| r.into_iter().map(|f|
            (f.perfect,f.inside,f.slot,f.fever,f.reference.to_bits(),f.rank)).collect()).collect())
    }
    #[test]
    fn rank_counts_keep_empty_outside_domains_and_inclusive_equal_frame_ties() {
        for extra_missing in [false,true] {
            let mut notes=vec![ReplayNote {time:99./120.,trigger:Some(0),
                window:NoteWindow::PinnedPerfect {frame:99}},
                ReplayNote {time:100./120.,trigger:None,
                    window:NoteWindow::NativeGreen {first_frames:[100,106,106,106],last_frame:105}}];
            if extra_missing {notes.push(ReplayNote {time:150./120.,trigger:None,
                window:NoteWindow::NativeGreen {first_frames:[150,156,156,156],last_frame:155}});}
            let input=ReplayInput {power:300040.75,play_level:27,fps:120,
                hit_frames:notes.iter().map(|n|(n.time*120.).round() as u32).collect(),notes,
                skills:vec![ReplaySkill {duration:0.1,correction:None,
                    score:ScoreEffect::Plain {percent:100.}}],initial_combo:699,medley_combo:true,
                fever_frames:Some([100,105]),constraints:vec![],hit_order:vec![],native_time_routing:None};
            let prepared=Prepared::new(&input).unwrap();
            for request in [0,99,100,101,140,149,160] {for playing in [0,1,6,60] {
                for correction in 0..=3 {for outside in 0..=3 {
                    let rules=||RowRules {references:None,outside_correction:outside,trigger_before:true};
                    for indexed in [false,true] {
                        prepared.envelope_cache.lock().unwrap().indexed=indexed;
                        assert_eq!(bits(prepared.sparse_net_envelope(0,request,playing,correction,rules())),
                            bits(prepared.net_envelope_legacy(0,request,playing,correction,rules())));
                    }
                }}
            }}
            let rules=||RowRules {references:None,outside_correction:0,trigger_before:true};
            assert_eq!(prepared.sparse_net_envelope(0,99,6,3,rules()).is_some(),!extra_missing);
            assert!(prepared.sparse_net_envelope(0,160,6,3,rules()).is_none());
        }
    }
    #[test]
    fn sparse_windows_keep_full_rank_domains_release_clipping_and_every_factor_bit() {
        let notes:Vec<_>=(0..12).map(|i| {
            let frame=120+i*4;
            ReplayNote {time:f64::from(frame)/120.,trigger:(i%2==0).then_some(i as usize/2),
                window:if i%5==1 {NoteWindow::NativeGreen {first_frames:[frame-18,frame-10,frame-5,frame],last_frame:frame+12}}
                else if i%5==3 {NoteWindow::PinnedPerfect {frame}} else {NoteWindow::Standard}}
        }).collect();
        let input=ReplayInput {power:300040.75,play_level:27,fps:120,notes,
            hit_frames:(0..12).map(|i|120+i*4).collect(),skills:vec![ReplaySkill {
                duration:0.1,correction:None,score:ScoreEffect::Plain {percent:100.}};6],
            initial_combo:699,medley_combo:true,fever_frames:Some([129,153]),
            constraints:vec![],hit_order:vec![],native_time_routing:None};
        for input in [input,crate::hit_timing::chain_tests::fixture()] {
            let prepared=Prepared::new(&input).unwrap();
            let references:Vec<_>=(0..input.notes.len()).map(|i| (i+1) as f64*0.001377).collect();
            for trigger in input.notes.iter().enumerate().filter_map(|(i,n)| n.trigger.map(|_|i)) {
                for request in [0,27,100,120,124,130,160,280,600] { for playing in [0,1,17,60] {
                    for correction in 0..=3 { for outside in 0..=3 { for before in [false,true] {
                        let rules=|| RowRules { references:Some(&references),outside_correction:outside,trigger_before:before };
                        for indexed in [false,true] {
                            prepared.envelope_cache.lock().unwrap().indexed=indexed;
                            assert_eq!(bits(prepared.sparse_net_envelope(trigger,request,playing,correction,rules())),
                                bits(prepared.net_envelope_legacy(trigger,request,playing,correction,rules())),
                                "trigger {trigger}, request {request}, playing {playing}, corrections {correction}/{outside}, before {before}, indexed {indexed}");
                        }
                    } } }
                } }
            }
            assert_eq!(prepared.envelope_cache.lock().unwrap().bases.iter().flatten().count(),4);
        }
    }
}
