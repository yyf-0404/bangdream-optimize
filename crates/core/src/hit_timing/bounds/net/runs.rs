//! Equal frame coefficients form runs, while original frame ordinals retain
//! the exact option order. Only independent options are compressed; stateful
//! event solvers still consume every original frame.
use super::{corrected, Judgement, OptionValue};
use crate::hit_timing::bounds::replacement::frames::Frame;

struct Run {
    first: u32,
    raw: Judgement,
    weight: [f64; 2],
    ordinals: Vec<usize>,
    ordinal: usize,
}
impl Run {
    fn last(&self) -> u32 { self.first + self.ordinals.len() as u32 - 1 }
    fn ordinal(&self, first: u32, last: u32) -> usize {
        if first == self.first && last == self.last() { return self.ordinal; }
        self.ordinals[(first - self.first) as usize..=(last - self.first) as usize]
            .iter().copied().min().unwrap()
    }
}

pub(super) struct NoteRuns {
    runs: Vec<Run>,
    first: u32,
    last: u32,
}
impl NoteRuns {
    pub(super) fn new(frames: &[Frame]) -> Self {
        let mut ordered: Vec<_> = frames.iter().enumerate().collect();
        ordered.sort_unstable_by_key(|&(ordinal, f)| (f.time, ordinal));
        let first = ordered.first().unwrap().1.time;
        let last = ordered.last().unwrap().1.time;
        let mut runs = Vec::<Run>::new();
        for (ordinal, f) in ordered {
            if let Some(run) = runs.last_mut() {
                if run.last().checked_add(1) == Some(f.time)
                    && run.raw == f.raw
                    && run.weight.map(f64::to_bits) == f.weight.map(f64::to_bits)
                {
                    run.ordinals.push(ordinal);
                    run.ordinal = run.ordinal.min(ordinal);
                    continue;
                }
            }
            runs.push(Run { first: f.time, raw: f.raw, weight: f.weight,
                ordinals: vec![ordinal], ordinal });
        }
        Self { runs, first, last }
    }
    pub(super) fn possible(&self, request: u32, last: u32, correction: u32) -> Option<u32> {
        self.runs.iter().find_map(|r| {
            let first = r.first.max(request + 1);
            (first <= r.last().min(last) && corrected(r.raw, correction).is_some())
                .then_some(first)
        })
    }
    pub(super) fn outside(&self, request: u32, last: u32) -> bool {
        self.first <= request || self.last > last
    }
    pub(super) fn constant_gain(&self, request:u32, last:u32, correction:u32,
        values:[f32;2], baseline:f64, evaluations:&mut u64) -> Option<f64> {
        let outside=self.outside(request,last);
        let mut best=if outside {0.} else {f64::NEG_INFINITY};
        let mut found=false;
        for run in &self.runs {
            if run.first.max(request+1)>run.last().min(last) {continue;}
            let Some(grade)=corrected(run.raw,correction) else {continue;};
            found=true;*evaluations+=1;
            let judged=if grade==0 {Judgement::Great} else {Judgement::Perfect};
            let weight=(run.weight[1]*f64::from(judged.multiplier())).next_up();
            let gain=((weight*f64::from(values[grade])).next_up()-baseline).next_up();
            best=best.max(gain);
        }
        // The original row omits an impossible optional note entirely, so
        // even a rounded addition of zero must be omitted for this case.
        (found || !outside).then_some(best)
    }
    pub(super) fn options(&self, request: u32, last: u32, correction: u32,
        firsts: &[u32], evaluations: &mut u64) -> Vec<OptionValue> {
        let mut options = Vec::<(usize, OptionValue)>::new();
        for run in &self.runs {
            let mut first = run.first.max(request + 1);
            let last = run.last().min(last);
            if first > last { continue; }
            let Some(grade) = corrected(run.raw, correction) else { continue; };
            let judged = if grade == 0 { Judgement::Great } else { Judgement::Perfect };
            let weight = (run.weight[1] * f64::from(judged.multiplier())).next_up();
            loop {
                *evaluations += 1;
                let count = firsts.partition_point(|&t| t <= first);
                let end = firsts.get(count).map_or(last, |&t| last.min(t - 1));
                let previous = count.saturating_sub(1);
                let ordinal = run.ordinal(first, end);
                if let Some((seen, value)) = options.iter_mut()
                    .find(|(_, o)| o.grade == grade && o.previous == previous)
                {
                    *seen = (*seen).min(ordinal);
                    value.weight = value.weight.max(weight);
                } else {
                    options.push((ordinal, OptionValue { grade, weight, previous }));
                }
                if end == last { break; }
                first = end + 1;
            }
        }
        options.sort_unstable_by_key(|(ordinal, _)| *ordinal);
        options.into_iter().map(|(_, o)| o).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constant_runs_match_rank_split_options_with_zero_gains_and_mandatory_empty_domains() {
        let frames:Vec<_>=(0..50).filter(|&f|f%7!=3).map(|time|Frame {time,
            raw:[Judgement::Bad,Judgement::Good,Judgement::Great,Judgement::Perfect][(time/13) as usize],
            weight:[0.00071,if time>=26 {0.0012} else {0.00091}]}).rev().collect();
        let runs=NoteRuns::new(&frames);
        let firsts=[0,4,4,7,11,19,19,19,26,38,55];
        for request in 0..55 {for width in [0,1,6,15,80] {for correction in 0..=3 {
            for values in [[0.,2.],[1.,1.],[1.1,2.35],[2.1,2.1]] {
                for baseline in [0.,0.00001,0.0031] {
                    let last=request+width;
                    let outside=runs.outside(request,last);
                    let options=runs.options(request,last,correction,&firsts,&mut 0);
                    let expected=if options.is_empty() && outside {None} else {Some(options.iter()
                        .map(|o|((o.weight*f64::from(values[o.grade])).next_up()-baseline).next_up())
                        .fold(if outside {0.} else {f64::NEG_INFINITY},f64::max))};
                    assert_eq!(runs.constant_gain(request,last,correction,values,baseline,&mut 0)
                        .map(f64::to_bits),expected.map(f64::to_bits));
                }
            }
        }}}
    }
    #[test]
    fn runs_preserve_options_and_first_appearance_for_shuffled_sparse_domains() {
        let mut seed = 0x6917324_u32;
        for sample in 0..40 {
            let mut frames: Vec<_> = (0..50).filter(|&time| (time + sample) % 11 != 0)
                .map(|time| Frame { time, raw: [Judgement::Bad, Judgement::Great,
                    Judgement::Perfect][(time / 13) as usize % 3],
                    weight: [0.00071, if time >= 26 { 0.0012 } else { 0.00091 }] })
                .collect();
            for i in (1..frames.len()).rev() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let j = seed as usize % (i + 1);
                frames.swap(i, j);
            }
            let runs = NoteRuns::new(&frames);
            for request in 0..55 { for width in [1, 6, 15, 80] { for correction in 0..=3 {
                let last = request + width;
                let inside = |f: &&Frame| request < f.time && f.time <= last;
                let possible = frames.iter().filter(inside)
                    .filter(|f| corrected(f.raw, correction).is_some()).map(|f| f.time).min();
                assert_eq!(runs.possible(request, last, correction), possible);
                assert_eq!(runs.outside(request, last),
                    frames.iter().any(|f| f.time <= request || f.time > last));
                let firsts = [1, 4, 4, 7, 11, 19, 19, 19, 26, 38, 55];
                let mut expected = Vec::<OptionValue>::new();
                for f in frames.iter().filter(inside) {
                    let Some(grade) = corrected(f.raw, correction) else { continue; };
                    let judged = if grade == 0 { Judgement::Great } else { Judgement::Perfect };
                    let weight = (f.weight[1] * f64::from(judged.multiplier())).next_up();
                    let previous = firsts.partition_point(|&t| t <= f.time).saturating_sub(1);
                    if let Some(o) = expected.iter_mut().find(|o| o.grade == grade && o.previous == previous) {
                        o.weight = o.weight.max(weight);
                    } else { expected.push(OptionValue { grade, weight, previous }); }
                }
                let actual = runs.options(request, last, correction, &firsts, &mut 0);
                assert_eq!(actual.len(), expected.len());
                for (a, b) in actual.iter().zip(&expected) {
                    assert_eq!((a.grade, a.weight.to_bits(), a.previous),
                        (b.grade, b.weight.to_bits(), b.previous));
                }
            } } }
        }
    }
}
