//! Bounded, fixed-origin image access for client-side header compositing.
use std::{
    collections::VecDeque,
    io::Read,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const MAX_IMAGE: usize = 4 * 1024 * 1024;
const MAX_CACHE: usize = 8 * 1024 * 1024;
type Entry = (String, Instant, Vec<u8>);
static CACHE: OnceLock<Mutex<VecDeque<Entry>>> = OnceLock::new();

pub fn allowed_path(path: &str) -> bool {
    let p: Vec<_> = path.split('/').collect();
    if p.first() != Some(&"assets")
        || !matches!(p.get(1), Some(&"jp" | &"cn" | &"en" | &"tw" | &"kr"))
    {
        return false;
    }
    if p.iter().any(|v| {
        v.is_empty()
            || *v == "."
            || *v == ".."
            || !v
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
    }) {
        return false;
    }
    match p.as_slice() {
        [_, _, "characters", "resourceset", resource, "card_normal.png" | "card_after_training.png"] => {
            resource.ends_with("_rip")
        }
        [_, _, "event", _, "topscreen_rip", "bg_eventtop.png" | "trim_eventtop.png"] => true,
        [_, _, "event", _, "images_rip", "logo.png"] => true,
        _ => false,
    }
}

pub fn fetch(path: &str) -> Result<Vec<u8>, String> {
    if !allowed_path(path) {
        return Err("Unsupported header asset path".into());
    }
    let cache = CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Some(bytes) = cache
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .find(|(key, at, _)| key == path && at.elapsed() < Duration::from_secs(86400))
        .map(|(_, _, bytes)| bytes.clone())
    {
        return Ok(bytes);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("Mozilla/5.0 BanG-Dream-Optimize/0.3")
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(format!("https://bestdori.com/{path}"))
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_IMAGE as u64)
    {
        return Err("Header asset exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_IMAGE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_IMAGE || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Invalid PNG header asset".into());
    }
    let mut entries = cache.lock().map_err(|e| e.to_string())?;
    entries.retain(|(key, at, _)| key != path && at.elapsed() < Duration::from_secs(86400));
    while entries.len() >= 12
        || entries.iter().map(|(_, _, b)| b.len()).sum::<usize>() + bytes.len() > MAX_CACHE
    {
        entries.pop_front();
    }
    entries.push_back((path.to_owned(), Instant::now(), bytes.clone()));
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::allowed_path;
    #[test]
    fn accepts_only_header_pngs_on_known_servers() {
        assert!(allowed_path(
            "assets/cn/event/lisa_base/topscreen_rip/trim_eventtop.png"
        ));
        assert!(allowed_path(
            "assets/jp/characters/resourceset/res023088_rip/card_normal.png"
        ));
        for path in [
            "https://example.com/x",
            "assets/jp/../event/x/images_rip/logo.png",
            "assets/xx/event/x/images_rip/logo.png",
            "assets/jp/event/x/images_rip/logo.png?url=x",
            "assets/jp/api/player/1",
            "assets/jp/event/x/images_rip/other.png",
        ] {
            assert!(!allowed_path(path), "{path}");
        }
    }
}
