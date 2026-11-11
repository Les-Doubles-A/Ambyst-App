//! GitHub access with an on-disk ETag cache.
//!
//! The app list comes from `catalog.json`, published by a GitHub Action of this
//! repository on raw.githubusercontent.com: a single download per launch, not
//! counted against the GitHub API rate limit (60 requests per hour without a
//! token). The API is only used as a fallback and for repositories added by hand.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const OWNER: &str = "Les-Doubles-A";
const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/Les-Doubles-A/Ambyst-App/catalog/catalog.json";

#[derive(Clone, Debug)]
pub struct Repo {
    pub full_name: String,
    pub name: String,
    pub description: String,
    pub default_branch: String,
    /// Date of the last push, used to know when a missing icon is worth a new search.
    pub pushed_at: String,
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub page: String,
    pub windows: Option<Asset>,
    pub android: Option<Asset>,
}

#[derive(Deserialize)]
struct ApiRepo {
    full_name: String,
    name: String,
    description: Option<String>,
    default_branch: String,
    #[serde(default)]
    pushed_at: Option<String>,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    archived: bool,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

// ---------------------------------------------------------------- HTTP + cache

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    entries: HashMap<String, CacheEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
struct CacheEntry {
    etag: String,
    body: String,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let c = std::fs::read_to_string(cache_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Mutex::new(c)
    })
}

fn cache_path() -> std::path::PathBuf {
    crate::store::data_dir().join("github-cache.json")
}

pub fn save_cache() {
    let Ok(c) = cache().lock() else { return };
    if let Ok(json) = serde_json::to_string(&*c) {
        let _ = std::fs::create_dir_all(crate::store::data_dir());
        let tmp = cache_path().with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(tmp, cache_path());
        }
    }
}

/// GITHUB_TOKEN, or the token of the GitHub CLI if it is installed and logged in.
fn token() -> Option<&'static str> {
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            if let Ok(t) = std::env::var("GITHUB_TOKEN") {
                if !t.trim().is_empty() {
                    return Some(t.trim().to_string());
                }
            }
            let mut cmd = std::process::Command::new("gh");
            cmd.args(["auth", "token"]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let out = cmd.output().ok()?;
            let t = String::from_utf8(out.stdout).ok()?.trim().to_string();
            (out.status.success() && !t.is_empty()).then_some(t)
        })
        .as_deref()
}

pub fn get(url: &str) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    let mut req = ureq::get(url)
        .header("User-Agent", concat!("Ambyst-App/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json");
    if url.starts_with("https://api.github.com/") {
        if let Some(t) = token() {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
    }
    req
}

enum Fetch {
    Body(String),
    NotFound,
}

/// GET a GitHub API URL, using the cache for 304 responses and when offline.
fn api_get(url: &str) -> Result<Fetch, String> {
    let cached = cache().lock().ok().and_then(|c| c.entries.get(url).cloned());
    let mut req = get(url);
    if let Some(c) = &cached {
        req = req.header("If-None-Match", &c.etag);
    }
    match req.call() {
        Ok(mut resp) => {
            if resp.status() == 304 {
                if let Some(c) = cached {
                    return Ok(Fetch::Body(c.body));
                }
            }
            let etag = resp
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = resp
                .body_mut()
                .with_config()
                .limit(20 * 1024 * 1024)
                .read_to_string()
                .map_err(|_| "Réponse GitHub illisible".to_string())?;
            if let (Some(etag), Ok(mut c)) = (etag, cache().lock()) {
                c.entries.insert(url.to_string(), CacheEntry { etag, body: body.clone() });
            }
            Ok(Fetch::Body(body))
        }
        Err(ureq::Error::StatusCode(404)) => Ok(Fetch::NotFound),
        Err(e) => {
            // Rate limited or offline: fall back to the last known answer.
            if let Some(c) = cached {
                return Ok(Fetch::Body(c.body));
            }
            Err(match e {
                ureq::Error::StatusCode(403) | ureq::Error::StatusCode(429) => {
                    "Limite GitHub atteinte, réessayez plus tard".to_string()
                }
                ureq::Error::StatusCode(code) => format!("Erreur GitHub ({code})"),
                _ => "Pas de connexion à GitHub".to_string(),
            })
        }
    }
}

// ---------------------------------------------------------------- API calls

pub fn list_repos(owner: &str) -> Result<Vec<Repo>, String> {
    let mut all = Vec::new();
    for page in 1..=10 {
        let url = format!(
            "https://api.github.com/users/{owner}/repos?per_page=100&page={page}&sort=full_name"
        );
        let Fetch::Body(body) = api_get(&url)? else {
            return Err(format!("Compte GitHub « {owner} » introuvable"));
        };
        let repos: Vec<ApiRepo> =
            serde_json::from_str(&body).map_err(|_| "Réponse GitHub illisible".to_string())?;
        let n = repos.len();
        all.extend(repos.into_iter().filter(|r| !r.fork && !r.archived).map(|r| Repo {
            full_name: r.full_name,
            name: r.name,
            description: r.description.unwrap_or_default(),
            default_branch: r.default_branch,
            pushed_at: r.pushed_at.unwrap_or_default(),
        }));
        if n < 100 {
            break;
        }
    }
    Ok(all)
}

pub fn repo_info(full_name: &str) -> Result<Repo, String> {
    let url = format!("https://api.github.com/repos/{full_name}");
    let Fetch::Body(body) = api_get(&url)? else {
        return Err("Dépôt introuvable".into());
    };
    let r: ApiRepo =
        serde_json::from_str(&body).map_err(|_| "Réponse GitHub illisible".to_string())?;
    Ok(Repo {
        full_name: r.full_name,
        name: r.name,
        description: r.description.unwrap_or_default(),
        default_branch: r.default_branch,
        pushed_at: r.pushed_at.unwrap_or_default(),
    })
}

#[derive(Deserialize)]
struct ApiTree {
    tree: Vec<ApiTreeEntry>,
}

#[derive(Deserialize)]
struct ApiTreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

/// Every file path of a branch (ETag cached, so cheap once known).
pub fn repo_files(full_name: &str, branch: &str) -> Result<Vec<String>, String> {
    let url = format!("https://api.github.com/repos/{full_name}/git/trees/{branch}?recursive=1");
    let Fetch::Body(body) = api_get(&url)? else {
        return Ok(Vec::new());
    };
    let tree: ApiTree =
        serde_json::from_str(&body).map_err(|_| "Réponse GitHub illisible".to_string())?;
    Ok(tree.tree.into_iter().filter(|e| e.kind == "blob").map(|e| e.path).collect())
}

#[derive(Deserialize)]
struct ApiCatalog {
    repos: Vec<ApiCatalogRepo>,
}

#[derive(Deserialize)]
struct ApiCatalogRepo {
    full_name: String,
    name: String,
    #[serde(default)]
    description: String,
    default_branch: String,
    #[serde(default)]
    pushed_at: String,
    release: Option<ApiRelease>,
    #[serde(default)]
    images: Vec<String>,
}

/// One repository of the published catalog, with its latest release (`None` if it
/// has none) and the paths of its image files.
pub struct CatalogEntry {
    pub repo: Repo,
    pub release: Option<Release>,
    pub images: Vec<String>,
}

/// Every app of OWNER in one download, outside the API rate limit.
pub fn catalog() -> Result<Vec<CatalogEntry>, String> {
    let Fetch::Body(body) = api_get(CATALOG_URL)? else {
        return Err("Catalogue introuvable".into());
    };
    parse_catalog(&body)
}

fn parse_catalog(body: &str) -> Result<Vec<CatalogEntry>, String> {
    let cat: ApiCatalog =
        serde_json::from_str(body).map_err(|_| "Catalogue illisible".to_string())?;
    Ok(cat
        .repos
        .into_iter()
        .map(|r| CatalogEntry {
            repo: Repo {
                full_name: r.full_name,
                name: r.name,
                description: r.description,
                default_branch: r.default_branch,
                pushed_at: r.pushed_at,
            },
            release: r.release.map(to_release),
            images: r.images,
        })
        .collect())
}

/// `Ok(None)` when the repository has no published release.
pub fn latest_release(full_name: &str) -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{full_name}/releases/latest");
    let Fetch::Body(body) = api_get(&url)? else {
        return Ok(None);
    };
    let rel: ApiRelease =
        serde_json::from_str(&body).map_err(|_| "Réponse GitHub illisible".to_string())?;
    Ok(Some(to_release(rel)))
}

fn to_release(rel: ApiRelease) -> Release {
    let pick = |score: fn(&str) -> Option<u8>| {
        rel.assets
            .iter()
            .filter_map(|a| score(&a.name).map(|s| (s, a)))
            .max_by_key(|(s, _)| *s)
            .map(|(_, a)| Asset {
                name: a.name.clone(),
                url: a.browser_download_url.clone(),
                size: a.size,
            })
    };
    Release {
        tag: rel.tag_name.clone(),
        page: rel.html_url.clone(),
        windows: pick(windows_score),
        android: pick(|n| n.to_lowercase().ends_with(".apk").then_some(1)),
    }
}

/// Higher is better. None = not usable on Windows x64.
fn windows_score(name: &str) -> Option<u8> {
    let n = name.to_lowercase();
    // Compare whole words, so that "kiosque" is not mistaken for "ios".
    let foreign = [
        "linux", "macos", "darwin", "mac", "osx", "aarch64", "arm64", "android", "ios",
    ];
    if n.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| foreign.contains(&w)) {
        return None;
    }
    let windowsish = n.contains("win") || n.contains("x64") || n.contains("x86_64");
    if n.ends_with(".exe") {
        Some(if is_setup_name(&n) { 6 } else { 4 })
    } else if n.ends_with(".msi") {
        Some(5)
    } else if n.ends_with(".zip") {
        Some(if windowsish { 3 } else { 1 })
    } else {
        None
    }
}

pub fn is_setup_name(lower: &str) -> bool {
    lower.contains("setup") || lower.contains("install")
}

/// Product name hidden in an asset name: "SwiftPad_0.1.0_x64-setup.exe" -> "SwiftPad".
pub fn asset_product(name: &str) -> String {
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    let mut out = String::new();
    for part in stem.split(['-', '_', ' ', '.']) {
        let p = part.to_lowercase();
        let is_version = p.trim_start_matches('v').chars().next().is_some_and(|c| c.is_ascii_digit());
        if is_version || ["setup", "installer", "install", "x64", "win64", "windows", "portable"].contains(&p.as_str()) {
            break;
        }
        out.push_str(part);
    }
    let lower = out.to_lowercase();
    if let Some(i) = lower.find("setup") {
        out.truncate(i);
    }
    out
}

/// Compares two tags such as "v1.2.10" and "1.2.9". True if `latest` is newer.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    fn nums(t: &str) -> Vec<u64> {
        let mut v: Vec<u64> = t
            .trim_start_matches(|c: char| !c.is_ascii_digit())
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect();
        while v.len() > 1 && v.last() == Some(&0) {
            v.pop();
        }
        v
    }
    let (a, b) = (nums(latest), nums(installed));
    if a.is_empty() || b.is_empty() {
        return latest.trim_start_matches('v') != installed.trim_start_matches('v');
    }
    a > b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer("v1.2.10", "1.2.9"));
        assert!(!is_newer("v1.2.0", "v1.2.0"));
        assert!(!is_newer("v2.0.0", "2.0"));
        assert!(!is_newer("1.0", "1.1"));
        assert!(is_newer("nightly-b", "nightly-a"));
    }

    #[test]
    fn asset_choice() {
        assert!(windows_score("App-Setup-1.0.exe") > windows_score("App.msi"));
        assert!(windows_score("app.msi") > windows_score("app-win64.zip"));
        assert_eq!(windows_score("app-linux.zip"), None);
        assert_eq!(windows_score("app.apk"), None);
        assert_eq!(windows_score("kiosque-Setup-1.0.0.exe"), Some(6));
        assert_eq!(windows_score("app-macos.zip"), None);
        assert_eq!(windows_score("app_arm64-setup.exe"), None);
        assert_eq!(windows_score("checksums.txt"), None);
    }

    #[test]
    fn catalog_format() {
        // Same shape as the output of tools/build_catalog.py.
        let body = r#"{"repos": [
            {"default_branch": "main", "description": "", "full_name": "o/a", "images": ["icons/icon128.png"],
             "name": "a", "pushed_at": "2026-11-11T10:00:00Z",
             "release": {"assets": [{"browser_download_url": "https://x/a-Setup-1.0.1.exe", "name": "a-Setup-1.0.1.exe", "size": 3}],
                         "html_url": "https://x", "tag_name": "v1.0.1"}},
            {"default_branch": "main", "description": "lib", "full_name": "o/b", "images": [],
             "name": "b", "pushed_at": "", "release": null}
        ], "version": 1}"#;
        let list = parse_catalog(body).unwrap();
        assert_eq!(list.len(), 2);
        let rel = list[0].release.as_ref().unwrap();
        assert_eq!(rel.tag, "v1.0.1");
        assert_eq!(rel.windows.as_ref().unwrap().name, "a-Setup-1.0.1.exe");
        assert_eq!(list[0].images, ["icons/icon128.png"]);
        assert!(list[1].release.is_none());
    }

    #[test]
    fn product_names() {
        assert_eq!(asset_product("SwiftPad_0.1.0_x64-setup.exe"), "SwiftPad");
        assert_eq!(asset_product("GoldenHueSetup-2.2.0.exe"), "GoldenHue");
        assert_eq!(asset_product("FileConverter.exe"), "FileConverter");
        assert_eq!(asset_product("clean-peek-Setup-1.0.0.exe"), "cleanpeek");
        assert_eq!(asset_product("ComsALL-1.0.0-windows.zip"), "ComsALL");
    }
}
