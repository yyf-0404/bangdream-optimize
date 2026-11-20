//! Park the clock-stable part of a completed first-frame touch closure.
//! Geometry's TemporalBlock proves that every legal ordinary touch sequence
//! has already been explored, with no timestamp/binding writes. Clock-stable
//! states are closed under these ordinary transitions. Any later extension
//! of a parked prefix is therefore already represented by that first closure.
//! Unstable states continue through every frame and reunite before retirement.
use super::*;

pub(super) struct Parked {
    pub(super) resume: usize,
    pub(super) values: Vec<Option<Block>>,
    pub(super) count: usize,
}

impl Parked {
    pub(super) fn new<const RECORD:bool>(storage:&mut Storage<RECORD>,
        values:&mut [Option<Block>],block:&geometry::TemporalBlock,idle_nodes:&[usize],
        costs:&impl machine::ActionCosts,stats:&mut WindowStats) -> Option<Self> {
        let mut parked=vec![None;block.count];
        let mut count=0;
        for (node,value) in values.iter_mut().enumerate() {
            let Some(source)=value.take() else {continue};
            // The checked graph has no reachable action from this node until
            // resume, and its null paths reach exactly the certified target.
            // Native clocks are untouched on null edges, regardless of expiry.
            let (stable,unstable)=if idle_nodes.binary_search(&node).is_ok() {(Some(source),None)}
                else {storage.partition(source,|state|
                    costs.stable_interval(state,block.first_frame,block.last_frame))};
            *value=unstable;
            if let Some(stable)=stable {
                count+=storage.len(&stable);
                let destination=if block.permutation.is_empty() {node} else {block.permutation[node]};
                storage.merge(&mut parked[destination],stable);
            }
            if storage.exhausted {return None;}
        }
        if count==0 {return None;}
        if RECORD {
            stats.temporal_partial_intervals+=1;
            stats.temporal_parked_value_states+=count as u64;
            stats.temporal_partial_peak_states=stats.temporal_partial_peak_states.max(count);
        }
        Some(Self {resume:block.resume,values:parked,count})
    }

    pub(super) fn reunite<const RECORD:bool>(self,storage:&mut Storage<RECORD>,values:&mut [Option<Block>]) {
        debug_assert_eq!(self.values.len(),values.len());
        for (target,source) in values.iter_mut().zip(self.values) {
            if let Some(source)=source {storage.merge(target,source);}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Proofs;
    impl machine::ActionCosts for Proofs {
        fn stable_interval(&self,state:usize,_first:u32,_last:u32) -> bool {state%2==0}
        fn hit(&mut self,_:geometry::Factor,_:usize) -> Result<(usize,Value),ReplayError> {unreachable!()}
        fn run(&mut self,_:geometry::Factor,_:usize,_:usize) -> Result<(usize,Value),ReplayError> {unreachable!()}
    }

    #[test]
    fn partial_frontier_reunites_at_the_exact_permuted_geometry_nodes() {
        let mut storage=Storage::<true>::new(LIMIT);
        let mut values=vec![storage.intern(vec![(0,10),(1,20)]),storage.intern(vec![(2,30),(3,40)])];
        let block=geometry::TemporalBlock {start:0,after_first:1,resume:3,
            first_frame:10,last_frame:12,count:2,permutation:vec![1,0]};
        let mut stats=WindowStats::default();
        let parked=Parked::new(&mut storage,&mut values,&block,&[],&Proofs,&mut stats).unwrap();
        assert_eq!(parked.count,2);
        assert_eq!(storage.values(&values[0].unwrap()),[(1,0)]);
        assert_eq!(storage.values(&values[1].unwrap()),[(3,0)]);
        values.swap(0,1);
        parked.reunite(&mut storage,&mut values);
        let read=|b:Block|storage.values(&b).iter().map(|&(state,score)|
            (state,b.offset+score)).collect::<Vec<_>>();
        assert_eq!(read(values[0].unwrap()),[(2,30),(3,40)]);
        assert_eq!(read(values[1].unwrap()),[(0,10),(1,20)]);
    }
}
