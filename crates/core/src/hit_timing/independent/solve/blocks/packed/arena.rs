//! Immutable normalized value sets in a bounded, query-local arena. Numeric
//! handles replace allocations/refcounts; no state or score crosses queries.
use super::*;
use crate::scoring::ScoringError;
use std::num::NonZeroUsize;
mod kernel;
mod partial;
pub(super) use kernel::evaluate;
#[cfg(test)]
pub(super) use kernel::evaluate_controlled;
#[cfg(test)]
pub(super) use kernel::evaluate_with_idle;
pub(super) const LIMIT: usize = 2 * 1024 * 1024;
const EMPTY: usize = usize::MAX;

#[derive(Clone, Copy)]
struct Block { id: NonZeroUsize, offset: u64 }
impl Block {
    fn new(id: usize,offset:u64) -> Self {
        Self {id:NonZeroUsize::new(id+1).expect("bounded value arena handle"),offset}
    }
    #[inline(always)]
    fn id(self) -> usize {self.id.get()-1}
}
#[cfg(test)]
pub(super) fn initial_limit() -> usize {
    Storage::<true>::new(LIMIT).fixed_bytes()+8*std::mem::size_of::<Payload>()
        +8*std::mem::size_of::<(usize,u64)>()
}
#[derive(Clone, Copy)]
struct Payload {
    first: usize, last: usize, hash: u64, maximum: u64,
    uniform: Option<Option<machine::NativeUniformState>>, irreducible: bool,
    interval: Option<(u32,u32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lazy_merge_matches_independent_union_with_subsets_prefixes_zero_and_maximum() {
        use std::collections::BTreeMap;
        let old=vec![(0,9),(2,u64::MAX),(4,0),(7,7)];
        let states=[0,2,3,4,6,7];
        for packed in 1usize..3usize.pow(states.len() as u32) {
            let source:Vec<_>=states.iter().enumerate().filter_map(|(i,&state)|
                match packed/3usize.pow(i as u32)%3 {0=>None,1=>Some((state,0)),_=>Some((state,u64::MAX))}).collect();
            let mut expected:BTreeMap<_,_>=old.iter().copied().collect();
            let mut added=0;
            for &(state,score) in &source {
                if let Some(value)=expected.get_mut(&state) {*value=(*value).max(score);}
                else {expected.insert(state,score);added+=1;}
            }
            for lazy in [false,true] {
                let mut storage=Storage::<true>::new(LIMIT);
                storage.lazy_merges=lazy;
                let mut target=Some(storage.intern(old.clone()).unwrap());
                let source=storage.intern(source.clone()).unwrap();
                assert_eq!(storage.merge(&mut target,source),added);
                let actual=target.unwrap();
                assert_eq!(storage.values(&actual).iter().map(|&(state,score)|
                    (state,actual.offset+score)).collect::<BTreeMap<_,_>>(),expected);
                assert!(!storage.exhausted);
            }
        }
    }

    #[test]
    fn arena_handles_survive_shape_table_collision_and_capacity_failure() {
        assert_eq!(std::mem::size_of::<Option<Block>>(),16);
        let mut storage=Storage::<true>::new(LIMIT);
        let original=storage.intern(vec![(0,100),(1,105)]).unwrap();
        let shifted=storage.intern(vec![(0,500),(1,505)]).unwrap();
        assert_eq!(original.id,shifted.id);
        assert_eq!(shifted.offset,500);
        let slot=storage.payloads[original.id()].hash as usize&(SLOTS-1);
        for state in 2..20_000 {
            let block=storage.intern(vec![(state,0)]).unwrap();
            if storage.payloads[block.id()].hash as usize&(SLOTS-1)==slot { break; }
        }
        assert_ne!(storage.shapes[slot],original.id());
        assert_eq!(storage.values(&original),[(0,0),(1,5)]);
        let another=storage.intern(vec![(0,800),(1,805)]).unwrap();
        assert_ne!(another.id,original.id);
        assert_eq!(storage.values(&another),storage.values(&original));
        storage.limit=storage.bytes();
        let large=storage.intern((20_000..100_000).map(|s|(s,0)).collect());
        assert!(large.is_none());
        assert!(storage.exhausted);
        assert_eq!(storage.values(&original),[(0,0),(1,5)]);
        assert_eq!(storage.maximum(&original),105);
    }
}
#[derive(Clone, Copy)]
struct Cached { source: usize, operator: (u64, u8, usize), output: Option<Block>, shift: u64 }
#[derive(Clone, Copy)]
struct Reduced { source: usize, output: Option<Block>, shift: u64, removed: usize }
struct Storage<const RECORD: bool> {
    payloads: Vec<Payload>, entries: Vec<(usize,u64)>,
    shapes: Vec<usize>, operators: Vec<Option<Cached>>, reductions: Vec<Option<Reduced>>,
    scratch: Vec<(usize,u64)>, stats: NativeBlockStats,
    uniform: bool, static_operators: bool, inline_reductions: bool,
    frame_cells: bool,
    lazy_merges: bool,
    exhausted: bool, limit: usize,
}
impl<const RECORD: bool> Storage<RECORD> {
    #[cfg(test)]
    fn new(limit: usize) -> Self {
        Self::with_controls(limit, Controls::dynamic())
    }
    fn with_controls(limit: usize, controls: Controls) -> Self {
        Self { payloads: vec![], entries: vec![], shapes: vec![EMPTY; SLOTS],
            operators: vec![None; SLOTS],
            reductions: if !controls.enabled(Switch::BlockReductions) {
                vec![] } else { vec![None; SLOTS] },
            scratch: vec![], stats: NativeBlockStats::default(), exhausted: false, limit,
            uniform: controls.enabled(Switch::UniformBlocks),
            lazy_merges: controls.enabled(Switch::NativeKernelFastpaths),
            frame_cells: controls.enabled(Switch::NativeClockCells),
            static_operators: controls.enabled(Switch::StaticBlockOperators),
            inline_reductions: controls.enabled(Switch::InlineReductions) }
    }
    fn fixed_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.shapes.capacity()*std::mem::size_of::<usize>()
            + self.operators.capacity()*std::mem::size_of::<Option<Cached>>()
            + self.reductions.capacity()*std::mem::size_of::<Option<Reduced>>()
    }
    fn bytes(&self) -> usize {
        self.fixed_bytes() + self.payloads.capacity()*std::mem::size_of::<Payload>()
            + self.entries.capacity()*std::mem::size_of::<(usize,u64)>()
    }
    fn reserve(&mut self, count: usize) -> bool {
        let payloads = if self.payloads.len()==self.payloads.capacity() {
            self.payloads.capacity().max(4).saturating_mul(2)
        } else { self.payloads.capacity() };
        let needed=self.entries.len().saturating_add(count);
        let entries=if needed>self.entries.capacity() {
            self.entries.capacity().max(4).saturating_mul(2).max(needed)
        } else { self.entries.capacity() };
        let bytes=self.fixed_bytes().saturating_add(payloads.saturating_mul(std::mem::size_of::<Payload>()))
            .saturating_add(entries.saturating_mul(std::mem::size_of::<(usize,u64)>()));
        if bytes>self.limit { self.exhausted=true; return false; }
        if payloads>self.payloads.capacity() { self.payloads.reserve_exact(payloads-self.payloads.len()); }
        if entries>self.entries.capacity() { self.entries.reserve_exact(entries-self.entries.len()); }
        if self.bytes()>self.limit { self.exhausted=true; return false; }
        if RECORD { self.stats.retained_bytes=self.stats.retained_bytes.max(self.bytes()); }
        true
    }
    fn len(&self, block: &Block) -> usize {
        let payload=self.payloads[block.id()]; payload.last-payload.first
    }
    fn values(&self, block: &Block) -> &[(usize,u64)] {
        let payload=self.payloads[block.id()]; &self.entries[payload.first..payload.last]
    }
    fn maximum(&self, block: &Block) -> u64 { block.offset+self.payloads[block.id()].maximum }
    fn shifted(&self, block: Block, shift: u64) -> Result<Block, ReplayError> {
        let offset=block.offset.checked_add(shift).ok_or(ScoringError::TotalScoreOverflow)?;
        offset.checked_add(self.payloads[block.id()].maximum).ok_or(ScoringError::TotalScoreOverflow)?;
        Ok(Block { id:block.id,offset })
    }
    fn recycle(&mut self, mut entries: Vec<(usize,u64)>) {
        entries.clear();
        if entries.capacity()>self.scratch.capacity() { self.scratch=entries; }
    }
    fn intern(&mut self, mut entries: Vec<(usize,u64)>) -> Option<Block> {
        debug_assert!(!entries.is_empty());
        debug_assert!(entries.windows(2).all(|w|w[0].0<w[1].0));
        let offset=entries.iter().map(|e|e.1).min().unwrap();
        let mut hash=0xcbf2_9ce4_8422_2325u64;
        let mut maximum=0;
        if RECORD { self.stats.normalized_entries+=entries.len() as u64; }
        for (state,score) in &mut entries {
            *score-=offset; maximum=maximum.max(*score);
            hash=(hash^*state as u64).wrapping_mul(0x100_0000_01b3);
            hash=(hash^*score).wrapping_mul(0x100_0000_01b3);
        }
        hash^=hash>>32;
        let slot=hash as usize&(SLOTS-1);
        let id=self.shapes[slot];
        if id!=EMPTY {
            let payload=self.payloads[id];
            if payload.hash==hash && self.entries[payload.first..payload.last]==entries {
                if RECORD { self.stats.shape_hits+=1; }
                self.recycle(entries); return Some(Block::new(id,offset));
            }
        }
        if RECORD { self.stats.shape_misses+=1; }
        if !self.reserve(entries.len()) { self.recycle(entries); return None; }
        let id=self.payloads.len();
        let first=self.entries.len();
        self.entries.extend_from_slice(&entries);
        self.payloads.push(Payload { first,last:self.entries.len(),hash,maximum,
            uniform:None,irreducible:false,interval:None });
        self.shapes[slot]=id;
        self.recycle(entries);
        Some(Block::new(id,offset))
    }
    fn uniform(&mut self, block: &Block, costs: &impl machine::ActionCosts) -> Option<machine::NativeUniformState> {
        let payload=self.payloads[block.id()];
        if let Some(proof)=payload.uniform { return proof; }
        let proof=self.entries[payload.first..payload.last].iter().try_fold(None,|common,&(state,_)| {
            let mut current=costs.native_uniform_state(state)?;
            if let Some(old)=common {
                let old: machine::NativeUniformState=old;
                if old.class!=current.class { current.class=(usize::MAX,usize::MAX); }
                current.first=current.first.max(old.first); current.last=current.last.min(old.last);
            }
            Some(Some(current))
        }).flatten();
        self.payloads[block.id()].uniform=Some(proof); proof
    }
    fn apply(&mut self, source: &Block, index: usize, graph: &Graph,
        costs: &mut impl machine::ActionCosts) -> Result<Option<Block>,ReplayError> {
        let actual=costs.native_block_operator(index);
        let proof=actual.and_then(|(frame,_,_)| {
            if !(self.uniform||self.static_operators) { return None; }
            self.uniform(source,costs).filter(|p|p.first<=frame&&frame<=p.last)
        });
        let interval=actual.and_then(|(frame,_,_)| {
            if !(self.static_operators&&self.frame_cells) {return None;}
            self.interval(source,frame,costs)
        });
        let operator=actual.map(|(frame,raw,band)|
            (if let Some((first,_))=interval {(1u64<<33)|u64::from(first)}
             else if self.static_operators&&proof.is_some() {1u64<<32}
             else {u64::from(frame)},raw,band));
        let slot=operator.map(|key|super::super::Storage::<RECORD>::operator_index(source.id(),key));
        if let Some(slot)=slot {
            if let Some(cached)=self.operators[slot] {
                if cached.source==source.id() && Some(cached.operator)==operator {
                    if RECORD { self.stats.operator_hits+=1; }
                    return cached.output.map(|block|self.shifted(block,
                        source.offset.checked_add(cached.shift).ok_or(ScoringError::TotalScoreOverflow)?)).transpose();
                }
            }
            if RECORD { self.stats.operator_misses+=1; }
        }
        if self.uniform && proof.is_some_and(|p|p.class!=(usize::MAX,usize::MAX)) {
            let state=self.values(source)[0].0;
            if RECORD { self.stats.evaluated_entries+=1; }
            match costs.action_at(index,&graph.actions.get(index),state)? {
                None=>{
                    if RECORD { self.stats.uniform_operators+=1; self.stats.uniform_entries_saved+=self.len(source).saturating_sub(1) as u64; }
                    if self.static_operators { if let (Some(slot),Some(operator))=(slot,operator) {
                        self.store(source,slot,operator,None); } }
                    return Ok(None);
                }
                Some((next,add)) if next==state=>{
                    debug_assert_eq!(add.coefficient,0.);
                    if RECORD { self.stats.uniform_operators+=1; self.stats.uniform_entries_saved+=self.len(source).saturating_sub(1) as u64; }
                    let output=Some(self.shifted(*source,add.score)?);
                    if self.static_operators { if let (Some(slot),Some(operator))=(slot,operator) {
                        self.store(source,slot,operator,output); } }
                    return Ok(output);
                }
                _=>{}
            }
        }
        let payload=self.payloads[source.id()];
        let mut entries=std::mem::take(&mut self.scratch);
        for &(state,relative) in &self.entries[payload.first..payload.last] {
            if RECORD { self.stats.evaluated_entries+=1; }
            let Some((state,add))=costs.action_at(index,&graph.actions.get(index),state)? else {continue};
            debug_assert_eq!(add.coefficient,0.);
            let score=(source.offset+relative).checked_add(add.score).ok_or(ScoringError::TotalScoreOverflow)?;
            entries.push((state,score));
        }
        if !entries.windows(2).all(|w|w[0].0<w[1].0) {
            entries.sort_unstable_by_key(|e|e.0);
            let mut write=0;
            for read in 0..entries.len() {
                let entry=entries[read];
                if write>0&&entries[write-1].0==entry.0 { entries[write-1].1=entries[write-1].1.max(entry.1); }
                else { entries[write]=entry; write+=1; }
            }
            entries.truncate(write);
        }
        let output=if entries.is_empty() {self.recycle(entries);None} else {self.intern(entries)};
        if let (Some(slot),Some(operator))=(slot,operator) { self.store(source,slot,operator,output); }
        Ok(output)
    }
    fn interval(&mut self,source:&Block,frame:u32,costs:&impl machine::ActionCosts)
        -> Option<(u32,u32)> {
        let payload=self.payloads[source.id()];
        if let Some((first,last))=payload.interval.filter(|&(first,last)|first<=frame&&frame<=last) {
            return Some((first,last));
        }
        // The intersection is a cell of the union of every immutable state's
        // boundaries. Its first frame uniquely identifies the complete vector
        // operator for this source payload, including clock-changing cells.
        let interval=self.entries[payload.first..payload.last].iter()
            .try_fold((0,u32::MAX),|(first,last),&(state,_)| {
                let (a,b)=costs.native_frame_interval(state,frame)?;
                Some((first.max(a),last.min(b)))
            });
        self.payloads[source.id()].interval=interval;interval
    }
    fn store(&mut self, source: &Block, slot: usize, operator: (u64,u8,usize), output: Option<Block>) {
        self.operators[slot]=Some(Cached { source:source.id(),operator,
            shift:output.map_or(0,|b|b.offset-source.offset),output:output.map(|b|Block {offset:0,..b}) });
    }
    fn merge(&mut self, target: &mut Option<Block>, source: Block) -> usize {
        let Some(old)=target else {
            let count=self.len(&source); *target=Some(source);
            if RECORD { self.stats.shared_propagations+=1; } return count;
        };
        if old.id==source.id {
            old.offset=old.offset.max(source.offset);
            if RECORD {self.stats.equal_shape_merges+=1;} return 0;
        }
        let pa=self.payloads[old.id()]; let pb=self.payloads[source.id()];
        let a=&self.entries[pa.first..pa.last]; let b=&self.entries[pb.first..pb.last];
        if RECORD {self.stats.scanned_merge_entries+=(a.len()+b.len()) as u64;}
        let (mut i,mut j)=(0,0);
        if self.lazy_merges {
            // Probe until the first missing/improved state. Everything before
            // it is exactly the old prefix; materialize it only if needed.
            while j<b.len() {
                while i<a.len() && a[i].0<b[j].0 {i+=1;}
                if i==a.len() || a[i].0!=b[j].0 || source.offset+b[j].1>old.offset+a[i].1 {break;}
                i+=1;j+=1;
            }
            if j==b.len() {
                if RECORD {self.stats.dominated_merge_entries_saved+=a.len() as u64;}
                return 0;
            }
        }
        let mut entries=std::mem::take(&mut self.scratch);
        entries.extend(a[..i].iter().map(|&(state,score)|(state,old.offset+score)));
        let (mut added,mut changed)=(0,self.lazy_merges);
        while i<a.len()||j<b.len() {
            if j==b.len()||(i<a.len()&&a[i].0<b[j].0) {entries.push((a[i].0,old.offset+a[i].1));i+=1;}
            else if i==a.len()||b[j].0<a[i].0 {entries.push((b[j].0,source.offset+b[j].1));j+=1;added+=1;changed=true;}
            else {let before=old.offset+a[i].1;let after=before.max(source.offset+b[j].1);
                changed|=after!=before;entries.push((a[i].0,after));i+=1;j+=1;}
        }
        if changed { if let Some(value)=self.intern(entries) {*old=value;} }
        else {self.recycle(entries);} added
    }
    fn partition(&mut self,source:Block,mut stable:impl FnMut(usize)->bool)
        -> (Option<Block>,Option<Block>) {
        let values=self.values(&source);
        let stable_count=values.iter().filter(|&&(state,_)|stable(state)).count();
        if stable_count==0 {return (None,Some(source));}
        if stable_count==values.len() {return (Some(source),None);}
        let mut parked=Vec::with_capacity(stable_count);
        let mut live=Vec::with_capacity(values.len()-stable_count);
        for &(state,relative) in values {
            let value=(state,source.offset+relative);
            if stable(state) {parked.push(value);} else {live.push(value);}
        }
        (self.intern(parked),self.intern(live))
    }
    fn reduce(&mut self, block: &mut Option<Block>, scratch: &mut dominance::Scratch,
        costs: &impl machine::ActionCosts, stats: &mut WindowStats) -> usize {
        let Some(value)=*block else {return 0};
        if self.len(&value)<=1 {return 0;}
        if self.inline_reductions&&self.payloads[value.id()].irreducible {
            if RECORD {self.stats.reduction_hits+=1;} return 0;
        }
        let index=value.id()&(SLOTS-1);
        if let Some(cached)=self.reductions.get(index).copied().flatten().filter(|r|r.source==value.id()) {
            if RECORD {self.stats.reduction_hits+=1;stats.dominated_value_states+=cached.removed as u64;}
            if cached.removed!=0 {*block=cached.output.map(|b|Block {offset:value.offset+cached.shift,..b});}
            return cached.removed;
        }
        if RECORD {self.stats.reduction_builds+=1;self.stats.reduction_entries+=self.len(&value) as u64;}
        let removed=scratch.dominated(self.values(&value).iter().map(|&(state,relative)|
            (state,Value {score:value.offset+relative,coefficient:0.})),costs);
        let count=removed.len();
        if self.inline_reductions&&count==0 {self.payloads[value.id()].irreducible=true;}
        if RECORD {stats.dominated_value_states+=count as u64;}
        if count!=0 {
            let payload=self.payloads[value.id()];
            let mut entries=std::mem::take(&mut self.scratch);
            let mut removed=removed.iter().peekable();
            for &(state,relative) in &self.entries[payload.first..payload.last] {
                if removed.peek().is_some_and(|&&id|id==state) {removed.next();}
                else {entries.push((state,value.offset+relative));}
            }
            *block=if entries.is_empty() {self.recycle(entries);None} else {self.intern(entries)};
        }
        if !self.reductions.is_empty() {
            self.reductions[index]=Some(Reduced {source:value.id(),
                output:block.map(|b|Block {offset:0,..b}),shift:block.map_or(0,|b|b.offset-value.offset),removed:count});
        } count
    }
}
