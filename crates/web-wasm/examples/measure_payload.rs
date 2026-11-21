use bangdream_optimize_web_wasm::{calculate_payload, WebCalculationPayload};
use std::{env, fs::File, io::BufReader, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = env::args_os()
        .nth(1)
        .ok_or("expected a WebCalculationPayload JSON file")?;
    let input: WebCalculationPayload = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let start = Instant::now();
    let result = calculate_payload(input)?;
    println!(
        "{:.3} s, score {}, {} song(s), hit timing {}",
        start.elapsed().as_secs_f64(),
        result.total_score,
        result.songs.len(),
        result.hit_timing.is_some()
    );
    Ok(())
}
