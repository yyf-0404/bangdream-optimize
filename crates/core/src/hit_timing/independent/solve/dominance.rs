//! Remove strictly worse native-score prefixes only under an explicit future
//! simulation supplied by ActionCosts. Each call covers one geometry node.
use super::*;
use std::cmp::Reverse;

#[derive(Default)]
pub(super) struct Scratch {
    rows: Vec<((bool, usize, u32), Reverse<usize>, u64, usize)>,
    removed: Vec<usize>,
    planes: Vec<(usize,Reverse<usize>,Reverse<usize>,Reverse<u64>,usize)>,
    ends: Vec<usize>,
    maximum: Vec<Option<u64>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Proofs;
    impl machine::ActionCosts for Proofs {
        fn dominance_rank(&self, state: usize) -> Option<(usize, u32, usize)> {
            [Some((1, 60, 0)), Some((1, 60, 1)), Some((1, 60, 0)),
             Some((2, 60, 5)), Some((1, 61, 5)), None][state]
        }
        fn hit(&mut self, _: geometry::Factor, _: usize) -> Result<(usize, Value), ReplayError> {
            unreachable!()
        }
        fn run(&mut self, _: geometry::Factor, _: usize, _: usize) -> Result<(usize, Value), ReplayError> {
            unreachable!()
        }
    }

    #[test]
    fn strict_future_dominance_keeps_ties_distinct_clocks_and_unproved_states() {
        let mut scratch = Scratch::default();
        let values = [10, 10, 9, 1000, 1000, 0];
        let removed = scratch.dominated(values.into_iter().enumerate()
            .map(|(state, score)| (state, Value { score, coefficient: 0. })), &Proofs);
        assert_eq!(removed, &[2]);
    }
    struct Planes;
    impl machine::ActionCosts for Planes {
        fn dominance_plane(&self,state:usize)->Option<(usize,usize,usize)> {
            [Some((0,0,0)),Some((0,1,0)),Some((0,0,1)),Some((0,1,1)),
                Some((0,1,1)),Some((0,2,0)),Some((1,2,2)),None][state]
        }
        fn hit(&mut self,_:geometry::Factor,_:usize)->Result<(usize,Value),ReplayError>{unreachable!()}
        fn run(&mut self,_:geometry::Factor,_:usize,_:usize)->Result<(usize,Value),ReplayError>{unreachable!()}
    }
    #[test]
    fn two_coordinate_frontier_matches_pairwise_simulation_with_zero_ties_and_maximum() {
        use machine::ActionCosts;
        let mut scratch=Scratch::default();
        for packed in 0usize..3usize.pow(8) {
            let scores=std::array::from_fn::<_,8,_>(|i|[0,1,u64::MAX][packed/3usize.pow(i as u32)%3]);
            let expected:Vec<_>=(0..8).filter(|&i| {
                let Some((g,p,e))=Planes.dominance_plane(i) else{return false;};
                (0..8).any(|j| {
                    let Some((h,q,f))=Planes.dominance_plane(j) else{return false;};
                    i!=j && g==h && q>=p && f>=e && scores[j]>=scores[i]
                        && (q>p || f>e || scores[j]>scores[i] || j<i)
                })
            }).collect();
            assert_eq!(scratch.dominated(scores.into_iter().enumerate().map(|(state,score)|
                (state,Value {score,coefficient:0.})),&Planes),expected);
        }
    }

    struct BinaryPlanes;
    impl machine::ActionCosts for BinaryPlanes {
        fn dominance_plane(&self,state:usize)->Option<(usize,usize,usize)> {
            [Some((0,0,0)),Some((0,1,0)),Some((0,0,1)),Some((0,1,1)),
                Some((0,1,1)),Some((0,1,2)),Some((1,1,2)),None][state]
        }
        fn hit(&mut self,_:geometry::Factor,_:usize)->Result<(usize,Value),ReplayError>{unreachable!()}
        fn run(&mut self,_:geometry::Factor,_:usize,_:usize)->Result<(usize,Value),ReplayError>{unreachable!()}
    }
    #[test]
    fn binary_end_contours_match_pairwise_dominance_with_zero_ties_and_maximum() {
        use machine::ActionCosts;
        let mut scratch=Scratch::default();
        for packed in 0usize..3usize.pow(8) {
            let scores=std::array::from_fn::<_,8,_>(|i|[0,1,u64::MAX][packed/3usize.pow(i as u32)%3]);
            let expected:Vec<_>=(0..8).filter(|&i| {
                let Some((g,p,e))=BinaryPlanes.dominance_plane(i) else{return false;};
                (0..8).any(|j| {
                    let Some((h,q,f))=BinaryPlanes.dominance_plane(j) else{return false;};
                    i!=j && g==h && q>=p && f>=e && scores[j]>=scores[i]
                        && (q>p || f>e || scores[j]>scores[i] || j<i)
                })
            }).collect();
            assert_eq!(scratch.dominated(scores.into_iter().enumerate().map(|(state,score)|
                (state,Value {score,coefficient:0.})),&BinaryPlanes),expected);
            assert!(scratch.ends.is_empty() && scratch.maximum.is_empty());
        }
    }
}

impl Scratch {
    pub fn dominated(
        &mut self,
        entries: impl Iterator<Item = (usize, Value)>,
        costs: &impl machine::ActionCosts,
    ) -> &[usize] {
        self.rows.clear();
        self.removed.clear();
        self.planes.clear();
        for (state, value) in entries {
            if let Some((group,progress,end))=costs.dominance_plane(state) {
                self.planes.push((group,Reverse(progress),Reverse(end),Reverse(value.score),state));
                continue;
            }
            if let Some((queue, ready, strength)) = costs.dominance_rank(state) {
                self.rows.push(((false, queue, ready), Reverse(strength), value.score, state));
            }
            if let Some((queue, ready, strength)) = costs.dominance_rank_extra(state) {
                self.rows.push(((true, queue, ready), Reverse(strength), value.score, state));
            }
        }
        if !self.planes.is_empty() {
            self.planes.sort_unstable();
            // A two-state skill has only two end-time contours. Merge them
            // with one cursor; it needs neither compression nor a Fenwick tree.
            if self.planes.iter().any(|row|row.1.0>1) {
                self.ends.clear();
                self.ends.extend(self.planes.iter().map(|row|row.2.0));
                self.ends.sort_unstable();self.ends.dedup();
                self.maximum.resize(self.ends.len()+1,None);
            }
            let mut from=0;
            while from<self.planes.len() {
                let group=self.planes[from].0;
                let mut until=from+1;
                while until<self.planes.len() && self.planes[until].0==group {until+=1;}
                if self.planes[from].1.0<=1 {
                    self.binary_plane(from,until);
                } else {
                    self.maximum.fill(None);
                    for &(_,_,Reverse(end),Reverse(score),state) in &self.planes[from..until] {
                        let position=self.ends.len()-self.ends.partition_point(|&v|v<end);
                        let mut index=position;
                        let mut best:Option<u64>=None;
                        while index!=0 {
                            best=best.max(self.maximum[index]);
                            index-=index&index.wrapping_neg();
                        }
                        if best.is_some_and(|best|best>=score) {self.removed.push(state);}
                        let mut index=position;
                        while index<self.maximum.len() {
                            self.maximum[index]=self.maximum[index].max(Some(score));
                            index+=index&index.wrapping_neg();
                        }
                    }
                }
                from=until;
            }
        }
        self.rows.sort_unstable_by_key(|&(group, strength, _, _)| (group, strength));
        let mut previous = None;
        let mut best = 0;
        for &(group, _, score, state) in &self.rows {
            if previous != Some(group) {
                previous = Some(group);
                best = score;
            } else if score < best || (group.0 && score == best) {
                // Equal prefixes remain; no tie is discarded on this proof.
                self.removed.push(state);
            } else {
                best = score;
            }
        }
        self.removed.sort_unstable();
        self.removed.dedup();
        &self.removed
    }

    fn binary_plane(&mut self,from:usize,until:usize) {
        let middle=from+self.planes[from..until].partition_point(|row|row.1.0==1);
        let mut best:Option<u64>=None;
        for &(_,_,_,Reverse(score),state) in &self.planes[from..middle] {
            if best.is_some_and(|best|best>=score) {self.removed.push(state);}
            best=best.max(Some(score));
        }
        let mut cursor=from;
        let mut clean_best:Option<u64>=None;
        let mut dirty_best:Option<u64>=None;
        for &(_,_,Reverse(end),Reverse(score),state) in &self.planes[middle..until] {
            while cursor<middle && self.planes[cursor].2.0>=end {
                clean_best=clean_best.max(Some(self.planes[cursor].3.0));
                cursor+=1;
            }
            if clean_best.max(dirty_best).is_some_and(|best|best>=score) {
                self.removed.push(state);
            }
            dirty_best=dirty_best.max(Some(score));
        }
    }
}
