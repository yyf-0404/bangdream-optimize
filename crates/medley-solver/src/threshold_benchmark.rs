use crate::{
    exact, solve_medley_wide_with, solve_medley_with, MedleySolverInput, MedleySolverPreference,
    WideMedleySolverInput,
};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

const FIXTURE_ENV: &str = "BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_FIXTURE";
const SIZES_ENV: &str = "BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_BENCH_SIZES";
const REPEATS_ENV: &str = "BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_BENCH_REPEATS";
const KINDS_ENV: &str = "BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_BENCH_KINDS";
const ALGORITHMS_ENV: &str = "BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_BENCH_ALGORITHMS";
const DEFAULT_SIZES: &str = "4096";
const DEFAULT_KINDS: &str = "frontier,stratified,conflict";
const MAGIC: &[u8; 4] = b"BMS1";

#[test]
#[ignore = "single-core Release benchmark; requires BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_FIXTURE"]
fn benchmarks_candidate_count_threshold_from_captured_solver_input() {
    let fixture_path = fixture_path();
    let fixture = CapturedInput::read(&fixture_path).unwrap();
    let sizes = parse_sizes(
        &std::env::var(SIZES_ENV).unwrap_or_else(|_| DEFAULT_SIZES.to_owned()),
        fixture.len(),
    );
    let kinds = std::env::var(KINDS_ENV).unwrap_or_else(|_| DEFAULT_KINDS.to_owned());
    let repeats = std::env::var(REPEATS_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5)
        .max(1);
    let algorithms =
        std::env::var(ALGORITHMS_ENV).unwrap_or_else(|_| "exact,random-bucket".to_owned());
    let run_exact = algorithms.split(',').any(|value| value.trim() == "exact");
    let run_bucket = algorithms
        .split(',')
        .any(|value| value.trim() == "random-bucket");
    assert!(
        run_exact || run_bucket,
        "{ALGORITHMS_ENV} selected no algorithm"
    );

    eprintln!(
        "fixture,kind,candidates,current_best,exact_median_ms,exact_max_ms,exact_work,exact_found,random_bucket_median_ms,random_bucket_max_ms,random_bucket_found"
    );
    for size in sizes {
        for kind in kinds
            .split(',')
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
        {
            let indices = match kind {
                "frontier" => fixture.frontier_indices(size, None),
                "stratified" => fixture.frontier_indices(size, Some(0x7a31_9e42)),
                "conflict" => fixture.conflict_indices(size),
                unknown => panic!("unknown benchmark subset kind {unknown}"),
            };
            let input = fixture.subset(&indices);
            eprintln!(
                "running fixture={} kind={kind} candidates={} algorithms={algorithms}",
                fixture_path.display(),
                input.len(),
            );
            let exact = run_exact.then(|| benchmark_exact(&input, repeats));
            let bucket = run_bucket.then(|| benchmark_random_bucket(&input, repeats));
            eprintln!(
                "{},{kind},{},{},{},{},{},{},{},{},{}",
                fixture_path.display(),
                input.len(),
                input.current_best(),
                metric(&exact, |result| format!("{:.3}", result.median_ms)),
                metric(&exact, |result| format!("{:.3}", result.max_ms)),
                metric(&exact, |result| result.work.to_string()),
                metric(&exact, |result| result.found.to_string()),
                metric(&bucket, |result| format!("{:.3}", result.median_ms)),
                metric(&bucket, |result| format!("{:.3}", result.max_ms)),
                metric(&bucket, |result| result.found.to_string()),
            );
        }
    }
}

fn metric(
    result: &Option<BenchmarkResult>,
    format: impl FnOnce(&BenchmarkResult) -> String,
) -> String {
    result.as_ref().map(format).unwrap_or_default()
}

fn fixture_path() -> PathBuf {
    let path = std::env::var_os(FIXTURE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("set {FIXTURE_ENV} to a captured .bms file or directory"));
    if path.is_file() {
        return path;
    }
    fs::read_dir(&path)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "bms"))
        .max_by_key(|path| {
            fs::metadata(path)
                .map(|metadata| metadata.len())
                .unwrap_or_default()
        })
        .unwrap_or_else(|| panic!("no .bms fixture found in {}", path.display()))
}

fn parse_sizes(value: &str, maximum: usize) -> Vec<usize> {
    let mut sizes = value
        .split(',')
        .filter_map(|value| value.trim().parse::<usize>().ok())
        .filter(|&size| size > 0 && size <= maximum)
        .collect::<Vec<_>>();
    sizes.sort_unstable();
    sizes.dedup();
    assert!(
        !sizes.is_empty(),
        "no benchmark size is within fixture size {maximum}"
    );
    sizes
}

struct BenchmarkResult {
    median_ms: f64,
    max_ms: f64,
    work: u64,
    found: bool,
}

fn benchmark_exact(input: &CapturedInput, repeats: usize) -> BenchmarkResult {
    let mut elapsed = Vec::with_capacity(repeats);
    let mut expected = None;
    let mut work = 0;
    let mut found = false;
    for run in 0..=repeats {
        let started = Instant::now();
        let outcome = match input {
            CapturedInput::Narrow(input) => exact::profile_narrow(input).unwrap(),
            CapturedInput::Wide(input) => exact::profile_wide(input).unwrap(),
        };
        let current = outcome.best_indices.map(|_| outcome.best_score);
        if let Some(expected) = expected {
            assert_eq!(current, expected, "exact result changed between repeats");
        } else {
            expected = Some(current);
        }
        if run > 0 {
            elapsed.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        work = outcome.work;
        found = outcome.best_indices.is_some();
    }
    summarize(elapsed, work, found)
}

fn benchmark_random_bucket(input: &CapturedInput, repeats: usize) -> BenchmarkResult {
    let mut elapsed = Vec::with_capacity(repeats);
    let mut found = false;
    for run in 0..=repeats {
        let started = Instant::now();
        let result = match input {
            CapturedInput::Narrow(input) => {
                solve_medley_with(input, MedleySolverPreference::FastApproximate)
            }
            CapturedInput::Wide(input) => {
                solve_medley_wide_with(input, MedleySolverPreference::FastApproximate)
            }
        };
        found = result.is_ok();
        if run > 0 {
            elapsed.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    summarize(elapsed, 0, found)
}

fn summarize(mut elapsed: Vec<f64>, work: u64, found: bool) -> BenchmarkResult {
    elapsed.sort_by(f64::total_cmp);
    BenchmarkResult {
        median_ms: elapsed[elapsed.len() / 2],
        max_ms: *elapsed.last().unwrap(),
        work,
        found,
    }
}

enum CapturedInput {
    Narrow(MedleySolverInput),
    Wide(WideMedleySolverInput),
}

impl CapturedInput {
    fn read(path: &Path) -> std::io::Result<Self> {
        let data = fs::read(path)?;
        let mut input = Cursor::new(data);
        let mut magic = [0; 4];
        input.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid medley solver fixture magic",
            ));
        }
        let kind = read_u8(&mut input)?;
        let mut reserved = [0; 3];
        input.read_exact(&mut reserved)?;
        let current_best = read_i32(&mut input)?;
        let candidate_count = read_u64(&mut input)? as usize;
        let word_count = read_u64(&mut input)? as usize;

        match kind {
            0 => {
                let mut team_masks = Vec::with_capacity(candidate_count);
                for _ in 0..candidate_count {
                    team_masks.push(read_u64(&mut input)?);
                }
                let scores = read_scores(&mut input, candidate_count)?;
                Ok(Self::Narrow(MedleySolverInput {
                    current_best,
                    team_masks,
                    scores,
                }))
            }
            1 => {
                let mut team_masks = Vec::with_capacity(candidate_count);
                for _ in 0..candidate_count {
                    let mut words = Vec::with_capacity(word_count);
                    for _ in 0..word_count {
                        words.push(read_u64(&mut input)?);
                    }
                    team_masks.push(words);
                }
                let scores = read_scores(&mut input, candidate_count)?;
                Ok(Self::Wide(WideMedleySolverInput {
                    current_best,
                    team_masks,
                    scores,
                }))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown medley solver fixture mask kind",
            )),
        }
    }

    fn len(&self) -> usize {
        self.scores().len()
    }

    fn current_best(&self) -> i32 {
        match self {
            Self::Narrow(input) => input.current_best,
            Self::Wide(input) => input.current_best,
        }
    }

    fn scores(&self) -> &[[i32; 3]] {
        match self {
            Self::Narrow(input) => &input.scores,
            Self::Wide(input) => &input.scores,
        }
    }

    fn masks_overlap(&self, left: usize, right: usize) -> bool {
        match self {
            Self::Narrow(input) => input.team_masks[left] & input.team_masks[right] != 0,
            Self::Wide(input) => input.team_masks[left]
                .iter()
                .zip(&input.team_masks[right])
                .any(|(left, right)| left & right != 0),
        }
    }

    fn frontier_indices(&self, size: usize, shuffle_seed: Option<u64>) -> Vec<usize> {
        let scores = self.scores();
        let mut orders: [Vec<usize>; 3] = std::array::from_fn(|song| {
            let mut order = (0..scores.len()).collect::<Vec<_>>();
            order.sort_by_key(|&index| std::cmp::Reverse(scores[index][song]));
            if let Some(seed) = shuffle_seed {
                shuffle_rank_blocks(&mut order, seed ^ song as u64);
            }
            order
        });
        round_robin_unique(&mut orders, size)
    }

    fn conflict_indices(&self, size: usize) -> Vec<usize> {
        let scores = self.scores();
        let anchors: [usize; 3] = std::array::from_fn(|song| {
            (0..scores.len())
                .max_by_key(|&index| scores[index][song])
                .unwrap()
        });
        let overlap_count = (0..scores.len())
            .map(|index| {
                anchors
                    .iter()
                    .filter(|&&anchor| self.masks_overlap(index, anchor))
                    .count()
            })
            .collect::<Vec<_>>();
        let mut orders: [Vec<usize>; 3] = std::array::from_fn(|song| {
            let mut order = (0..scores.len()).collect::<Vec<_>>();
            order.sort_by_key(|&index| {
                std::cmp::Reverse((overlap_count[index], scores[index][song]))
            });
            order
        });
        round_robin_unique(&mut orders, size)
    }

    fn subset(&self, indices: &[usize]) -> Self {
        match self {
            Self::Narrow(input) => Self::Narrow(MedleySolverInput {
                current_best: input.current_best,
                team_masks: indices
                    .iter()
                    .map(|&index| input.team_masks[index])
                    .collect(),
                scores: indices.iter().map(|&index| input.scores[index]).collect(),
            }),
            Self::Wide(input) => Self::Wide(WideMedleySolverInput {
                current_best: input.current_best,
                team_masks: indices
                    .iter()
                    .map(|&index| input.team_masks[index].clone())
                    .collect(),
                scores: indices.iter().map(|&index| input.scores[index]).collect(),
            }),
        }
    }
}

fn round_robin_unique(orders: &mut [Vec<usize>; 3], size: usize) -> Vec<usize> {
    let maximum = orders[0].len();
    let target = size.min(maximum);
    let mut selected = Vec::with_capacity(target);
    let mut seen = vec![false; maximum];
    let mut positions = [0; 3];
    while selected.len() < target {
        let before = selected.len();
        for song in 0..3 {
            while positions[song] < orders[song].len() {
                let index = orders[song][positions[song]];
                positions[song] += 1;
                if !seen[index] {
                    seen[index] = true;
                    selected.push(index);
                    break;
                }
            }
            if selected.len() == target {
                break;
            }
        }
        assert_ne!(selected.len(), before, "unable to fill stratified subset");
    }
    selected
}

fn shuffle_rank_blocks(order: &mut [usize], seed: u64) {
    let mut rng = SplitMix64::new(seed);
    for block in order.chunks_mut(256) {
        for index in (1..block.len()).rev() {
            let other = (rng.next() % (index as u64 + 1)) as usize;
            block.swap(index, other);
        }
    }
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

fn read_u8(input: &mut impl Read) -> std::io::Result<u8> {
    let mut value = [0];
    input.read_exact(&mut value)?;
    Ok(value[0])
}

fn read_u64(input: &mut impl Read) -> std::io::Result<u64> {
    let mut value = [0; 8];
    input.read_exact(&mut value)?;
    Ok(u64::from_le_bytes(value))
}

fn read_i32(input: &mut impl Read) -> std::io::Result<i32> {
    let mut value = [0; 4];
    input.read_exact(&mut value)?;
    Ok(i32::from_le_bytes(value))
}

fn read_scores(input: &mut impl Read, count: usize) -> std::io::Result<Vec<[i32; 3]>> {
    let mut scores = Vec::with_capacity(count);
    for _ in 0..count {
        scores.push([read_i32(input)?, read_i32(input)?, read_i32(input)?]);
    }
    Ok(scores)
}
