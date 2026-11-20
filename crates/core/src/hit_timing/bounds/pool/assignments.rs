//! Equal gain rows are interchangeable in the assignment DP. The quotient
//! remembers how many members of each row have been used. Addition still
//! proceeds in trigger slot order, with the same outward rounding at each step.
use super::*;
pub(super) mod identities;

const SLOTS: usize = 256;
type Key = [[u64; 6]; 5];
struct Entry { key: Key, value: (f64,f64) }
#[derive(Clone, Default, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignmentStats {
    pub row_identity_queries: u64,
    pub row_identity_hits: u64,
    pub row_identity_builds: u64,
    pub queries: u64,
    pub hits: u64,
    pub builds: u64,
    pub transitions: u64,
    pub original_transitions: u64,
    pub retained_bytes: usize,
}
pub(super) struct Assignments {
    table: Vec<Option<Entry>>,
    enabled: bool,
    avalanche: bool,
    identities: identities::Cache,
    pub stats: AssignmentStats,
}
impl Default for Assignments {
    fn default() -> Self {
        Self {table:vec![],stats:Default::default(),identities:Default::default(),
            avalanche: std::env::var("BANDORI_TIMING_DISABLE_ASSIGNMENT_AVALANCHE").as_deref()!=Ok("1"), enabled:
            std::env::var("BANDORI_TIMING_DISABLE_GROUPED_ASSIGNMENTS").as_deref()!=Ok("1")}
    }
}
impl Assignments {
    pub fn sum(&mut self, choices: &[PoolTimingChoice;5]) -> (f64,f64) {
        self.stats.queries += 1;
        if !self.enabled {
            self.stats.original_transitions += 80;
            return original(choices);
        }
        if let Some(value)=self.identities.sum(choices,&mut self.stats) {
            self.stats.retained_bytes=self.stats.retained_bytes.max(self.identities.bytes()
                +self.table.capacity()*std::mem::size_of::<Option<Entry>>());
            return value;
        }
        let mut key = choices.map(|c| c.gains.map(f64::to_bits));
        // Member relabeling preserves the set of all five-slot assignments,
        // including the order of additions within each assignment.
        key.sort_unstable();
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for &v in key.iter().flatten() { hash=(hash^v).wrapping_mul(0x100_0000_01b3); }
        if self.avalanche {
            hash = (hash ^ (hash >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
            hash = (hash ^ (hash >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
            hash ^= hash >> 33;
        } else { hash ^= hash >> 32; }
        let index = hash as usize & (SLOTS-1);
        if let Some(entry) = self.table.get(index).and_then(Option::as_ref) {
            if entry.key == key { self.stats.hits+=1; return entry.value; }
        }
        self.stats.builds+=1;
        let value = grouped(&key,&mut self.stats.transitions);
        if self.table.is_empty() {
            self.table.resize_with(SLOTS,||None);
            self.stats.retained_bytes=self.table.capacity()*std::mem::size_of::<Option<Entry>>()
                +self.identities.bytes();
        }
        self.table[index]=Some(Entry {key,value});
        value
    }
}
fn original(choices:&[PoolTimingChoice;5])->(f64,f64) {
    let mut dp=[f64::NEG_INFINITY;32]; dp[0]=0.;
    for mask in 0usize..31 {
        let slot=mask.count_ones() as usize;
        for (member,c) in choices.iter().enumerate() {
            if mask & (1<<member)==0 {
                let next=mask | (1<<member);
                dp[next]=dp[next].max((dp[mask]+c.gains[slot]).next_up());
            }
        }
    }
    (dp[31],choices.iter().map(|c| c.gains[5]).fold(0.,f64::max))
}
fn grouped(key:&Key, transitions:&mut u64)->(f64,f64) {
    let pattern = key.windows(2).enumerate().fold(0, |mask, (i, pair)|
        mask | (usize::from(pair[0] != pair[1]) << i));
    static PROGRAMS: std::sync::OnceLock<[Program; 16]> = std::sync::OnceLock::new();
    let program = &PROGRAMS.get_or_init(|| std::array::from_fn(Program::new))[pattern];
    let mut dp=[f64::NEG_INFINITY;32];dp[0]=0.;
    for edge in &program.edges[..program.len] {
        let (from, to) = (usize::from(edge.from), usize::from(edge.to));
        dp[to] = dp[to].max((dp[from]
            + f64::from_bits(key[usize::from(edge.member)][usize::from(edge.slot)])).next_up());
    }
    *transitions += program.len as u64;
    (dp[program.states-1],key.iter().map(|c| f64::from_bits(c[5])).fold(0.,f64::max))
}
#[derive(Clone, Copy, Default)]
struct Edge { from: u8, to: u8, member: u8, slot: u8 }
struct Program { edges: [Edge; 80], len: usize, states: usize }
impl Program {
    fn new(pattern: usize) -> Self {
        let mut groups = [(0usize, 0usize, 0usize); 5];
        let (mut count, mut states) = (0, 1);
        for member in 0..5 {
            if member == 0 || pattern & (1 << (member-1)) != 0 {
                groups[count] = (member, 1, 0); count += 1;
            } else { groups[count-1].1 += 1; }
        }
        for group in &mut groups[..count] { group.2 = states; states *= group.1+1; }
        let mut result = Self { edges: [Edge::default(); 80], len: 0, states };
        for from in 0..states-1 {
            let slot = groups[..count].iter().map(|g| from/g.2 % (g.1+1)).sum::<usize>();
            for &(member, limit, stride) in &groups[..count] {
                if from/stride % (limit+1) < limit {
                    result.edges[result.len] = Edge { from: from as u8,
                        to: (from+stride) as u8, member: member as u8, slot: slot as u8 };
                    result.len += 1;
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_multiplicity_and_member_relabeling_preserves_coefficient_bits() {
        let rows:[[f64;6];5]=std::array::from_fn(|i| std::array::from_fn(|s|
            ((i+1)*(s+3)) as f64*0.137-0.75));
        for packed in 0usize..3125 {
            let mut index=packed;
            let choices:[PoolTimingChoice;5]=std::array::from_fn(|_| {
                let row=rows[index%5];index/=5;
                PoolTimingChoice {gains:row,..Default::default()}
            });
            let expected=original(&choices);
            let mut key=choices.map(|c| c.gains.map(f64::to_bits));key.sort_unstable();
            let mut transitions=0;
            let actual=grouped(&key,&mut transitions);
            assert_eq!(actual.0.to_bits(),expected.0.to_bits(),"assignment {packed}");
            assert_eq!(actual.1.to_bits(),expected.1.to_bits());
            assert!(transitions<=80);
        }
        let key=[[0.13_f64.to_bits();6];5]; let mut transitions=0;
        grouped(&key,&mut transitions); assert_eq!(transitions,5);
    }
    #[test]
    fn bounded_table_checks_complete_keys_and_never_reuses_power() {
        let mut cache=Assignments {enabled:true,..Default::default()};
        for i in 0..2048 {
            let mut choices=std::array::from_fn(|_| PoolTimingChoice {
                gains:[i as f64*0.127;6],linear_power:i as f64,..Default::default()});
            assert_eq!(cache.sum(&choices),original(&choices));
            choices[0].linear_power=1000.;choices.reverse();
            assert_eq!(cache.sum(&choices),original(&choices));
            assert_eq!(cache.stats.retained_bytes,SLOTS*std::mem::size_of::<Option<Entry>>());
        }
        assert_eq!(cache.stats.hits,2048);
    }
}
