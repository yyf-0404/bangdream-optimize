//! A native clock is unobserved on graph paths containing no action. Certify
//! both absence of reachable actions and the exact null-path destination.
use super::*;

#[derive(Clone,Copy,Default)]
struct Reach {
    action:bool,
    destination:Option<usize>,
    ambiguous:bool,
}
impl Reach {
    fn join(&mut self,other:Self) {
        self.action|=other.action;
        self.ambiguous|=other.ambiguous;
        if let Some(destination)=other.destination {
            if self.destination.is_some_and(|old|old!=destination) {self.ambiguous=true;}
            else {self.destination=Some(destination);}
        }
    }
}

pub(super) fn nodes(layers:&[geometry::Layer],touches:&[geometry::TouchDag],
    block:&geometry::TemporalBlock)->Vec<usize> {
    if block.after_first==0 || block.after_first>=block.resume || block.resume>layers.len()
        || (!block.permutation.is_empty() && block.permutation.len()!=block.count)
        || layers[block.after_first-1].count!=block.count || layers[block.resume-1].count!=block.count {
        return vec![];
    }
    let mut current:Vec<_>=(0..block.count).map(|destination|
        Reach {destination:Some(destination),..Reach::default()}).collect();
    let mut previous=vec![];
    for index in (block.after_first..block.resume).rev() {
        let layer=&layers[index];
        if current.len()!=layer.count {return vec![];}
        let count=layers[index-1].count;
        previous.resize(count,Reach::default());previous.fill(Reach::default());
        if let Ok(position)=touches.binary_search_by_key(&index,|touch|touch.layer) {
            let touch=&touches[position];
            if touch.incremental.is_some() || touch.inputs.len()!=count {return vec![];}
            // Touch edges run in topological order over the current frame's
            // array. Reverse that exact order; zero-touch roots remain present.
            for edge in layer.edges.iter().rev() {
                if edge.from>=current.len() || edge.to>=current.len() {return vec![];}
                if edge.action.is_some() {current[edge.from].action=true;}
                else {let target=current[edge.to];current[edge.from].join(target);}
            }
            for (source,&target) in touch.inputs.iter().enumerate() {
                let Some(&reach)=current.get(target) else{return vec![];};
                previous[source]=reach;
            }
        } else {
            for edge in &layer.edges {
                if edge.from>=previous.len() || edge.to>=current.len() {return vec![];}
                if edge.action.is_some() {previous[edge.from].action=true;}
                else {previous[edge.from].join(current[edge.to]);}
            }
        }
        std::mem::swap(&mut current,&mut previous);
    }
    current.into_iter().enumerate().filter_map(|(source,reach)| {
        let target=if block.permutation.is_empty() {source} else {block.permutation[source]};
        (!reach.action && !reach.ambiguous && reach.destination==Some(target)).then_some(source)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use geometry::{Layer,Edge,TemporalBlock,TouchDag};
    fn block(count:usize,resume:usize,permutation:Vec<usize>)->TemporalBlock {
        TemporalBlock {start:0,after_first:1,resume,count,first_frame:1,last_frame:3,permutation}
    }
    #[test]
    fn idle_paths_preserve_permutation_and_reject_unreachable_or_ambiguous_carries() {
        let layers=[Layer {count:2,edges:vec![].into()},Layer {count:2,edges:vec![
            Edge {from:0,to:0,action:None},Edge {from:1,to:1,action:None},
            Edge {from:0,to:1,action:Some(0)}].into()},Layer {count:2,edges:vec![
            Edge {from:0,to:1,action:None},Edge {from:1,to:0,action:None}].into()}];
        assert_eq!(nodes(&layers,&[],&block(2,3,vec![1,0])),vec![1]);
        assert!(nodes(&layers,&[],&block(2,3,vec![])).is_empty());
        let layers=[Layer {count:2,edges:vec![].into()},Layer {count:2,edges:vec![
            Edge {from:0,to:0,action:None},Edge {from:0,to:1,action:None}].into()}];
        assert!(nodes(&layers,&[],&block(2,2,vec![])).is_empty());
    }
    #[test]
    fn touch_roots_reject_even_dead_end_hits_and_keep_complete_zero_touch_paths() {
        let layers=[Layer {count:2,edges:vec![].into()},Layer {count:3,edges:vec![
            Edge {from:0,to:2,action:Some(0)}].into()},Layer {count:2,edges:vec![
            Edge {from:0,to:0,action:None},Edge {from:1,to:1,action:None}].into()}];
        let touches=[TouchDag {layer:1,inputs:vec![0,1],incremental:None}];
        assert_eq!(nodes(&layers,&touches,&block(2,3,vec![])),vec![1]);
    }
}
