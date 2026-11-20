//! The old envelope takes a maximum per note. Equal raw grade and upper
//! weight bits give the exact same candidate throughout a contiguous run.
use super::*;
struct Run { first: u32, last: u32, raw: Judgement, weight: f64 }
pub(super) struct Runs { runs: Vec<Run> }
impl Runs {
    pub(super) fn new(frames: &[frames::Frame]) -> Self {
        let mut ordered: Vec<_>=frames.iter().collect();
        ordered.sort_unstable_by_key(|f| f.time);
        let mut runs=Vec::<Run>::new();
        for f in ordered {
            if let Some(run)=runs.last_mut().filter(|r| r.last.checked_add(1)==Some(f.time)
                && r.raw==f.raw && r.weight.to_bits()==f.weight[1].to_bits()) {
                run.last=f.time;
            } else { runs.push(Run { first:f.time,last:f.time,raw:f.raw,weight:f.weight[1] }); }
        }
        Self { runs }
    }
    pub(super) fn candidates(&self, start: u32, end: u32)
        -> impl Iterator<Item=(Judgement,f64)> + '_ {
        self.runs.iter().filter(move |r| start<r.last && r.first<=end)
            .map(|r| (r.raw,r.weight))
    }
    pub(super) fn boundaries(&self) -> impl Iterator<Item=(u32,u32)> + '_ {
        self.runs.iter().map(|r| (r.first,r.last))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compressed_maxima_match_sparse_shuffled_frames_at_every_boundary() {
        let mut fs:Vec<_>=(1..=70).filter(|t| t%11!=0).map(|time| frames::Frame {
            time,raw:[Judgement::Bad,Judgement::Good,Judgement::Great,Judgement::Perfect]
                [(time/9) as usize%4],weight:[0.001,if time<35 {0.0137} else {0.0201}] }).collect();
        fs.reverse(); let runs=Runs::new(&fs);
        for start in 0..75 { for width in [1,3,20,80] { for correction in 0..=3 {
            for factors in [[1.,1.1],[1.137_f32 as f64,2.913_f32 as f64]] {
                let value=|(raw,weight):(Judgement,f64)| frames::corrected(raw,correction)
                    .map(|g| up(up(up(weight*f64::from(if g==0 {Judgement::Great}
                        else {Judgement::Perfect}.multiplier()))*factors[g])-0.0171));
                let expected=fs.iter().filter(|f| start<f.time && f.time<=start+width)
                    .filter_map(|f| value((f.raw,f.weight[1]))).fold(0_f64,f64::max);
                let actual=runs.candidates(start,start+width).filter_map(value).fold(0_f64,f64::max);
                assert_eq!(actual.to_bits(),expected.to_bits());
            }
        } } }
    }
}
