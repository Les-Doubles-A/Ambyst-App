use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    /// Installed by running a setup program (.exe / .msi).
    Installer,
    /// Copied or extracted into Ambyst-App's own apps folder.
    Portable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    /// "owner/name"
    pub repo: String,
    #[serde(default)]
    pub installed_tag: Option<String>,
    #[serde(default)]
    pub kind: Option<Kind>,
    #[serde(default)]
    pub exe_path: Option<PathBuf>,
}

impl Entry {
    pub fn new(repo: String) -> Self {
        Self { repo, installed_tag: None, kind: None, exe_path: None }
    }

    pub fn name(&self) -> &str {
        self.repo.rsplit('/').next().unwrap_or(&self.repo)
    }

    pub fn key(&self) -> String {
        self.repo.to_lowercase()
    }
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

pub fn load() -> Vec<Entry> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

pub fn save(entries: &[Entry]) {
    let path = store_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(entries) {
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
