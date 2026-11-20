//! Private, never-recycled identities of immutable complete gain rows. Unknown
//! synthetic rows keep the full-bit-key path. No power or exact score is stored.
use super::*;
use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicU64,Ordering}};
const SLOTS: usize=1024;
static NEXT: AtomicU64=AtomicU64::new(1);

pub(in crate::hit_timing::bounds::pool) fn rows(gains: &[[f64;6]]) -> Arc<Vec<u64>> {
    let mut known=HashMap::new();
    Arc::new(gains.iter().map(|row| *known.entry(row.map(f64::to_bits)).or_insert_with(||
        NEXT.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|id|id.checked_add(1))
            .unwrap_or(0))).collect())
}
struct Entry {key:[u64;5],value:(f64,f64)}
pub(super) struct Cache {table:Vec<Option<Entry>>,enabled:bool,direct_keys:bool}
impl Default for Cache {
    fn default() -> Self {
        Self {table:vec![],enabled:
            std::env::var("BANDORI_TIMING_DISABLE_ASSIGNMENT_ROW_IDS").as_deref()!=Ok("1"),
            direct_keys:std::env::var("BANDORI_TIMING_DISABLE_DIRECT_ROW_KEYS").as_deref()!=Ok("1")}
    }
}
impl Cache {
    pub(super) fn bytes(&self) -> usize {
        self.table.capacity()*std::mem::size_of::<Option<Entry>>()
    }
    pub(super) fn sum(&mut self,choices:&[PoolTimingChoice;5],stats:&mut AssignmentStats)
        -> Option<(f64,f64)> {
        if !self.enabled || choices.iter().any(|c|c.assignment_row==0) {return None;}
        stats.row_identity_queries+=1;
        // Hits need only the complete immutable row identities. Sorting the
        // five integers avoids repeated indirect reads of the wide choices.
        let key=if self.direct_keys {
            let mut key=choices.map(|choice|choice.assignment_row);
            key.sort_unstable();key
        } else {
            let mut order=[0usize,1,2,3,4];
            order.sort_unstable_by_key(|&i|choices[i].assignment_row);
            order.map(|i|choices[i].assignment_row)
        };
        let mut hash=0xcbf2_9ce4_8422_2325u64;
        for id in key {hash=(hash^id).wrapping_mul(0x100_0000_01b3);}
        hash=(hash^(hash>>33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        hash=(hash^(hash>>33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        let index=(hash^(hash>>33)) as usize&(SLOTS-1);
        if let Some(entry)=self.table.get(index).and_then(Option::as_ref).filter(|e|e.key==key) {
            stats.hits+=1;stats.row_identity_hits+=1;return Some(entry.value);
        }
        stats.builds+=1;stats.row_identity_builds+=1;
        // Reordering member labels preserves every slot-order sum. The native
        // directed rounding of each addition is performed by the same program.
        // Materialize the full gain rows only on a miss, in exactly the same
        // ID order as before. Duplicate IDs denote bit-identical gain rows.
        let rows=key.map(|id|choices.iter().find(|choice|choice.assignment_row==id)
            .expect("assignment row came from a complete choice").gains.map(f64::to_bits));
        let value=grouped(&rows,&mut stats.transitions);
        if self.table.is_empty() {self.table.resize_with(SLOTS,||None);}
        self.table[index]=Some(Entry {key,value});
        stats.retained_bytes=stats.retained_bytes.max(self.bytes());
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_id_assignments_match_full_bits_for_all_rosters_and_owners() {
        let gains=[[-0.,0.,0.13,0.2,0.3,1.],
            [0.,-0.,0.2,0.3,0.4,0.7],
            [0.7,-0.1,0.2,0.4,0.3,0.6],
            [0.13,0.2,0.3,0.4,0.5,0.6]];
        let ids=rows(&gains);
        let other=rows(&gains);
        assert_ne!(ids[0],other[0]);
        let duplicate=rows(&[gains[0],gains[0]]);
        assert_eq!(duplicate[0],duplicate[1]);
        let mut cache=Assignments {enabled:true,..Default::default()};
        cache.identities.enabled=true;
        let mut indirect=Assignments {enabled:true,..Default::default()};
        indirect.identities.enabled=true;indirect.identities.direct_keys=false;
        for packed in 0usize..4usize.pow(5) {
            let choices=std::array::from_fn(|i| {
                let row=(packed>>(i*2))&3;
                PoolTimingChoice {gains:gains[row],assignment_row:
                    if i%2==0 {ids[row]} else {other[row]},..Default::default()}
            });
            let expected=original(&choices);
            let actual=cache.sum(&choices);
            assert_eq!(actual,indirect.sum(&choices));
            assert_eq!(actual.0.to_bits(),expected.0.to_bits());
            assert_eq!(actual.1.to_bits(),expected.1.to_bits());
            let mut changed=choices;changed.reverse();changed[0].linear_power=9876.54321;
            assert_eq!(cache.sum(&changed),expected);
        }
        assert_eq!(cache.stats.row_identity_queries,2048);
        assert!(cache.stats.row_identity_hits>=1024);
        assert!(cache.identities.bytes()<=SLOTS*std::mem::size_of::<Option<Entry>>());
    }

    #[test]
    fn union_and_minimum_keep_only_bit_identical_complete_rows() {
        let gains=[[1.;6],[2.;6],[1.,2.,1.,2.,1.,2.],[-0.;6],[0.;6]];
        let ids=rows(&gains);
        let choice=|i|PoolTimingChoice {assignment_row:ids[i],gains:gains[i],..Default::default()};
        assert_eq!(choice(0).union(choice(1)).assignment_row,ids[1]);
        assert_eq!(choice(0).minimum_components(choice(1)).assignment_row,ids[0]);
        let mut a=choice(2);a.gains=[2.,1.,2.,1.,2.,1.];a.assignment_row=0;
        assert_eq!(choice(2).union(a).assignment_row,0);
        assert_eq!(choice(2).minimum_components(a).assignment_row,0);
        for merged in [choice(3).union(choice(4)),choice(3).minimum_components(choice(4))] {
            let i=ids.iter().position(|&id|id==merged.assignment_row).unwrap();
            assert_eq!(merged.gains.map(f64::to_bits),gains[i].map(f64::to_bits));
        }
    }
}
