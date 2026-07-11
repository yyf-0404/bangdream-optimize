use crate::{MedleySolverInput, WideMedleySolverInput};
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

const CAPTURE_DIR_ENV: &str = "BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_CAPTURE_DIR";
const CAPTURE_MIN_ENV: &str = "BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_CAPTURE_MIN_CANDIDATES";
const MAGIC: &[u8; 4] = b"BMS1";
const NARROW_KIND: u8 = 0;
const WIDE_KIND: u8 = 1;

pub(crate) fn maybe_capture_narrow(input: &MedleySolverInput) {
    let Some(directory) = capture_directory(input.scores.len()) else {
        return;
    };
    let mut hasher = DefaultHasher::new();
    input.current_best.hash(&mut hasher);
    input.team_masks.hash(&mut hasher);
    input.scores.hash(&mut hasher);
    let path = capture_path(&directory, "narrow", input.scores.len(), hasher.finish());
    capture_file(&path, || write_narrow(&path, input));
}

pub(crate) fn maybe_capture_wide(input: &WideMedleySolverInput) {
    let Some(directory) = capture_directory(input.scores.len()) else {
        return;
    };
    let mut hasher = DefaultHasher::new();
    input.current_best.hash(&mut hasher);
    input.team_masks.hash(&mut hasher);
    input.scores.hash(&mut hasher);
    let path = capture_path(&directory, "wide", input.scores.len(), hasher.finish());
    capture_file(&path, || write_wide(&path, input));
}

fn capture_directory(candidate_count: usize) -> Option<PathBuf> {
    let directory = std::env::var_os(CAPTURE_DIR_ENV).map(PathBuf::from)?;
    let minimum = std::env::var(CAPTURE_MIN_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or_default();
    (candidate_count >= minimum).then_some(directory)
}

fn capture_path(directory: &Path, kind: &str, candidate_count: usize, hash: u64) -> PathBuf {
    directory.join(format!("{kind}-{candidate_count}-{hash:016x}.bms"))
}

fn capture_file(path: &Path, write: impl FnOnce() -> std::io::Result<()>) {
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            eprintln!(
                "medley solver capture skipped: path={} error={error}",
                path.display()
            );
            return;
        }
    }
    if let Err(error) = write() {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return;
        }
        let _ = fs::remove_file(path);
        eprintln!(
            "medley solver capture failed: path={} error={error}",
            path.display()
        );
    }
}

fn writer(path: &Path) -> std::io::Result<BufWriter<std::fs::File>> {
    let file = OpenOptions::new().write(true).create_new(true).open(path)?;
    Ok(BufWriter::new(file))
}

fn write_header(
    output: &mut impl Write,
    kind: u8,
    current_best: i32,
    candidate_count: usize,
    word_count: usize,
) -> std::io::Result<()> {
    output.write_all(MAGIC)?;
    output.write_all(&[kind])?;
    output.write_all(&[0; 3])?;
    output.write_all(&current_best.to_le_bytes())?;
    output.write_all(&(candidate_count as u64).to_le_bytes())?;
    output.write_all(&(word_count as u64).to_le_bytes())
}

fn write_scores(output: &mut impl Write, scores: &[[i32; 3]]) -> std::io::Result<()> {
    for score in scores {
        for value in score {
            output.write_all(&value.to_le_bytes())?;
        }
    }
    Ok(())
}

fn write_narrow(path: &Path, input: &MedleySolverInput) -> std::io::Result<()> {
    let mut output = writer(path)?;
    write_header(
        &mut output,
        NARROW_KIND,
        input.current_best,
        input.scores.len(),
        1,
    )?;
    for mask in &input.team_masks {
        output.write_all(&mask.to_le_bytes())?;
    }
    write_scores(&mut output, &input.scores)?;
    output.flush()
}

fn write_wide(path: &Path, input: &WideMedleySolverInput) -> std::io::Result<()> {
    let mut output = writer(path)?;
    let word_count = input.team_masks.first().map(Vec::len).unwrap_or_default();
    write_header(
        &mut output,
        WIDE_KIND,
        input.current_best,
        input.scores.len(),
        word_count,
    )?;
    for mask in &input.team_masks {
        for word in mask {
            output.write_all(&word.to_le_bytes())?;
        }
    }
    write_scores(&mut output, &input.scores)?;
    output.flush()
}
