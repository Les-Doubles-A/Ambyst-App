use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Installed by running a setup program (.exe / .msi).
    Installer,
    /// Copied or extracted into Ambyst-App's own apps folder.
    Portable,
}

/// What Ambyst-App itself installed (used when Windows has no record of the app).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Install {
    pub tag: String,
    pub kind: Kind,
    #[serde(default)]
    pub exe_path: Option<PathBuf>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Data {
    /// Repositories added by hand, in addition to the catalog ("owner/name").
    #[serde(default)]
    pub extra_repos: Vec<String>,
    /// Keyed by lowercase "owner/name".
    #[serde(default)]
    pub installs: BTreeMap<String, Install>,
}

/// Format written by version 1.0.0.
#[derive(Deserialize)]
struct OldEntry {
    repo: String,
    #[serde(default)]
    installed_tag: Option<String>,
    #[serde(default)]
    kind: Option<Kind>,
    #[serde(default)]
    exe_path: Option<PathBuf>,
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Ambyst-App")
}

pub fn apps_dir() -> PathBuf {
    data_dir().join("apps")
}

fn store_path() -> PathBuf {
    data_dir().join("apps.json")
}

pub fn load() -> Data {
    let Ok(raw) = std::fs::read_to_string(store_path()) else {
        return Data::default();
    };
    let raw = raw.trim_start_matches('\u{feff}');
    if let Ok(d) = serde_json::from_str::<Data>(raw) {
        return d;
    }
    let mut data = Data::default();
    if let Ok(old) = serde_json::from_str::<Vec<OldEntry>>(raw) {
        for e in old {
            if !e.repo.to_lowercase().starts_with(&format!("{}/", crate::github::OWNER.to_lowercase())) {
                data.extra_repos.push(e.repo.clone());
            }
            if let (Some(tag), Some(kind)) = (e.installed_tag, e.kind) {
                data.installs
                    .insert(e.repo.to_lowercase(), Install { tag, kind, exe_path: e.exe_path });
            }
        }
    }
    data
}

pub fn save(data: &Data) {
    let path = store_path();
    let _ = std::fs::create_dir_all(data_dir());
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// Accepts "owner/name", "github.com/owner/name", full URLs, with or without ".git".
pub fn parse_repo(input: &str) -> Option<String> {
    let mut s = input.trim();
    for prefix in ["https://", "http://", "www.", "github.com/"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
        }
    }
    let mut parts = s.split('/').filter(|p| !p.is_empty());
    let owner = parts.next()?;
    let name = parts.next()?.trim_end_matches(".git");
    let valid = |p: &str| {
        !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    if valid(owner) && valid(name) {
        Some(format!("{owner}/{name}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::parse_repo;

    #[test]
    fn parses_common_forms() {
        assert_eq!(parse_repo("a/b").as_deref(), Some("a/b"));
        assert_eq!(parse_repo("https://github.com/a/b.git").as_deref(), Some("a/b"));
        assert_eq!(parse_repo("github.com/a/b/releases").as_deref(), Some("a/b"));
        assert_eq!(parse_repo("  https://www.github.com/a-1/b_2/ ").as_deref(), Some("a-1/b_2"));
        assert_eq!(parse_repo("nope"), None);
        assert_eq!(parse_repo("a/b c"), None);
    }
}
