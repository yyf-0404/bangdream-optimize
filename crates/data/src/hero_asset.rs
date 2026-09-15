//! Bounded, fixed-origin image access for header compositing and result snapshots.
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
    if let ["res", "icon", name] = p.as_slice() {
        return matches!(
            *name,
            "powerful.svg" | "cool.svg" | "happy.svg" | "pure.svg"
        ) || numbered_icon(name, "band_", ".svg")
            || numbered_icon(name, "star_", ".png")
            || numbered_icon(name, "chara_icon_", ".png");
    }
    if p.first() != Some(&"assets")
        || !matches!(p.get(1), Some(&"jp" | &"cn" | &"en" | &"tw" | &"kr"))
    {
        return false;
    }
    match p.as_slice() {
        [_, _, "characters", "resourceset", resource, "card_normal.png" | "card_after_training.png"] => {
            resource.ends_with("_rip")
        }
        [_, _, "event", _, "topscreen_rip", "bg_eventtop.png" | "trim_eventtop.png"] => true,
        [_, _, "event", _, "images_rip", "logo.png"] => true,
        [_, _, "thumb", "chara", group, file] => {
            group.starts_with("card")
                && group.ends_with("_rip")
                && (file.ends_with("_normal.png") || file.ends_with("_after_training.png"))
        }
        [_, _, "thumb", "areaitem", "group00000_rip", file] => {
            numbered_icon(file, "areaItemRes", ".png")
        }
        [_, _, "musicjacket", group, file] => {
            group.starts_with("musicjacket")
                && group.ends_with("_rip")
                && file.starts_with("assets-star-forassetbundle-startapp-musicjacket-")
                && file.ends_with("-jacket.png")
        }
        _ => false,
    }
}

fn numbered_icon(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|s| s.strip_suffix(suffix))
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

pub fn content_type(path: &str) -> &'static str {
    if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/png"
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
    let valid = if content_type(path) == "image/svg+xml" {
        std::str::from_utf8(&bytes).is_ok_and(|text| text.contains("<svg"))
    } else {
        bytes.starts_with(b"\x89PNG\r\n\x1a\n")
    };
    if bytes.len() > MAX_IMAGE || !valid {
        return Err("Invalid image asset".into());
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
            "res/icon/band_1.svg", "res/icon/powerful.svg", "res/icon/star_5.png",
            "assets/jp/thumb/chara/card00000_rip/res001_normal.png",
            "assets/cn/thumb/areaitem/group00000_rip/areaItemRes00080.png",
            "assets/jp/musicjacket/musicjacket10_rip/assets-star-forassetbundle-startapp-musicjacket-musicjacket10-song-jacket.png",
        ] { assert!(allowed_path(path), "{path}"); }
        for path in [
            "https://example.com/x",
            "res/icon/../../api/player/1",
            "res/icon/band_1.svg?url=http://localhost",
            "res/icon/unlisted.svg",
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
