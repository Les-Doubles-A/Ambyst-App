//! App icons, read from the repository files through raw.githubusercontent.com
//! (not counted against the GitHub API rate limit) and cached on disk.

use crate::github::Repo;
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

/// True if a recent search found nothing and the repository has not been pushed since.
pub fn recently_missing(repo: &Repo) -> bool {
    let file = cache_file(&repo.full_name, "none");
    let fresh = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < RETRY_MISSING_AFTER);
    fresh && std::fs::read_to_string(&file).is_ok_and(|stamp| stamp == repo.pushed_at)
}

/// Looks for an icon in the usual places of a repository, then anywhere in its
/// file tree. `images` is the list of image files given by the catalog; without
/// it, the usual places are probed blindly and the tree is read from the API.
/// Returns RGBA SIZE x SIZE.
pub fn fetch(repo: &Repo, images: Option<&[String]>) -> Option<Vec<u8>> {
    let (full_name, name, branch) = (&repo.full_name, &repo.name, &repo.default_branch);
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
    let mut tried: Vec<String> = Vec::new();
    for path in candidates {
        if images.is_some_and(|list| !list.contains(&path)) {
            continue;
        }
        if let Some(rgba) = download(full_name, branch, &path) {
            return Some(save(full_name, rgba));
        }
        tried.push(path);
    }
    // Not at a usual place: rank every image of the repository.
    let files = match images {
        Some(list) => Ok(list.to_vec()),
        None => crate::github::repo_files(full_name, branch),
    };
    if let Ok(files) = files {
        let mut ranked: Vec<(i32, String)> = files
            .into_iter()
            .filter(|p| !tried.contains(p))
            .filter_map(|p| score(&p, &lower).map(|s| (s, p)))
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        for (_, path) in ranked.into_iter().take(5) {
            if let Some(rgba) = download(full_name, branch, &path) {
                return Some(save(full_name, rgba));
            }
        }
    }
    let file = cache_file(full_name, "none");
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, &repo.pushed_at);
    None
}

fn download(full_name: &str, branch: &str, path: &str) -> Option<Vec<u8>> {
    let url = format!("https://raw.githubusercontent.com/{full_name}/{branch}/{path}");
    let resp = crate::github::get(&url).call().ok()?;
    let bytes = resp.into_body().with_config().limit(4 * 1024 * 1024).read_to_vec().ok()?;
    decode(&bytes)
}

fn save(full_name: &str, rgba: Vec<u8>) -> Vec<u8> {
    let file = cache_file(full_name, "rgba");
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&file, &rgba);
    let _ = std::fs::remove_file(cache_file(full_name, "none"));
    rgba
}

/// How likely a file of the repository is to be the app icon. `None` if it is not a
/// usable image or clearly something else (splash screen, test fixture...).
fn score(path: &str, repo_lower: &str) -> Option<i32> {
    let lower = path.to_lowercase();
    let png = lower.ends_with(".png");
    if !png && !lower.ends_with(".ico") {
        return None;
    }
    const SKIP: [&str; 14] = [
        "node_modules/", "target/", "dist/", "build/", ".github/", "test", "fixture",
        "screenshot", "splash", "monochrome", "foreground", "background", "banner", "docs/",
    ];
    if SKIP.iter().any(|s| lower.contains(s)) {
        return None;
    }
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let mut score = 0;
    if stem.contains("icon") || stem.contains("ic_launcher") || stem.starts_with("ic_") {
        score += 30;
    }
    if stem.contains("logo") {
        score += 25;
    }
    if !repo_lower.is_empty() && stem.contains(repo_lower) {
        score += 30;
    }
    if stem.contains("round") {
        score -= 5;
    }
    for (hint, bonus) in [
        ("512", 12), ("256", 12), ("xxxhdpi", 12), ("192", 10), ("128", 10), ("xxhdpi", 8),
        ("xhdpi", 4), ("16", -10), ("mdpi", -5),
    ] {
        if lower.contains(hint) {
            score += bonus;
            break;
        }
    }
    if png {
        score += 5;
    }
    score -= lower.matches('/').count() as i32;
    Some(score)
}

fn decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = img.resize_exact(SIZE, SIZE, image::imageops::FilterType::Lanczos3);
    Some(img.to_rgba8().into_raw())
}

#[cfg(test)]
mod tests {
    use super::score;

    #[test]
    fn icon_ranking() {
        fn best<'a>(files: &[&'a str], repo: &str) -> Option<&'a str> {
            files.iter().max_by_key(|f| score(f, repo).unwrap_or(i32::MIN)).copied()
        }
        let android = [
            "android/app/src/main/res/drawable/splash.png",
            "android/app/src/main/res/mipmap-mdpi/ic_launcher.png",
            "android/app/src/main/res/mipmap-xxxhdpi/ic_launcher.png",
            "android/app/src/main/res/mipmap-xxxhdpi/ic_launcher_round.png",
            "android/app/src/main/res/mipmap-xxxhdpi/ic_launcher_foreground.png",
        ];
        assert_eq!(best(&android, "reveilly"), Some(android[2]));
        let win = ["windows/app/icons/icon.ico", "windows/app/icons/icon.png"];
        assert_eq!(best(&win, "filesend"), Some(win[1]));
        assert_eq!(score("README.md", "x"), None);
        assert_eq!(score("node_modules/a/icon.png", "x"), None);
    }
}
