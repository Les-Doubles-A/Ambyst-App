//! App icons, read from the repository files through raw.githubusercontent.com
//! (not counted against the GitHub API rate limit) and cached on disk.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub const SIZE: u32 = 64;
const RETRY_MISSING_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);

fn cache_file(full_name: &str, ext: &str) -> PathBuf {
    let safe: String = full_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' })
        .collect();
    crate::store::data_dir().join("icons").join(format!("{safe}.{ext}"))
}

/// Cached icon only, no network.
pub fn cached(full_name: &str) -> Option<Vec<u8>> {
    let bytes = std::fs::read(cache_file(full_name, "rgba")).ok()?;
    (bytes.len() == (SIZE * SIZE * 4) as usize).then_some(bytes)
}

/// True if a previous search found nothing recently.
pub fn recently_missing(full_name: &str) -> bool {
    std::fs::metadata(cache_file(full_name, "none"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < RETRY_MISSING_AFTER)
}

/// Looks for an icon in the usual places of a repository. Returns RGBA SIZE x SIZE.
pub fn fetch(full_name: &str, name: &str, branch: &str) -> Option<Vec<u8>> {
    let lower = name.to_lowercase();
    let candidates = [
        "ambyst-icon.png".to_string(),
        "assets/icon-256.png".into(),
        format!("{name}.png"),
        format!("{lower}.png"),
        format!("assets/{name}.png"),
        "assets/icon.png".into(),
        format!("resources/{name}.png"),
        "icons/icon128.png".into(),
        "extension/icons/icon128.png".into(),
        "src-tauri/icons/128x128@2x.png".into(),
        "src-tauri/icons/128x128.png".into(),
        "icon.png".into(),
        format!("{name}.ico"),
        format!("{lower}.ico"),
        format!("assets/{name}.ico"),
        format!("installer/{name}.ico"),
        format!("installer/{lower}.ico"),
        format!("desktop/{name}.ico"),
        "assets/icon.ico".into(),
        "icon.ico".into(),
    ];
    for path in candidates {
        let url = format!("https://raw.githubusercontent.com/{full_name}/{branch}/{path}");
        let Ok(resp) = crate::github::get(&url).call() else { continue };
        let Ok(bytes) = resp.into_body().with_config().limit(4 * 1024 * 1024).read_to_vec()
        else {
            continue;
        };
        if let Some(rgba) = decode(&bytes) {
            let file = cache_file(full_name, "rgba");
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&file, &rgba);
            let _ = std::fs::remove_file(cache_file(full_name, "none"));
            return Some(rgba);
        }
    }
    let file = cache_file(full_name, "none");
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, b"");
    None
}

fn decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3);
    Some(img.to_rgba8().into_raw())
}
