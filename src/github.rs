use serde::Deserialize;

#[derive(Clone, Debug)]
pub struct Release {
    pub tag: String,
    pub asset_name: String,
    pub asset_url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub fn get(url: &str) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    let mut req = ureq::get(url)
        .header("User-Agent", concat!("Ambyst-App/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
    }
    req
}

pub fn latest_release(repo: &str) -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let mut resp = get(&url).call().map_err(|e| match e {
        ureq::Error::StatusCode(404) => "Dépôt introuvable ou aucune release publiée".to_string(),
        ureq::Error::StatusCode(403) | ureq::Error::StatusCode(429) => {
            "Limite GitHub atteinte, réessayez plus tard".to_string()
        }
        ureq::Error::StatusCode(code) => format!("Erreur GitHub ({code})"),
        _ => "Pas de connexion à GitHub".to_string(),
    })?;
    let rel: ApiRelease = resp
        .body_mut()
        .read_json()
        .map_err(|_| "Réponse GitHub illisible".to_string())?;

    let asset = rel
        .assets
        .iter()
        .filter_map(|a| asset_score(&a.name).map(|s| (s, a)))
        .max_by_key(|(s, _)| *s)
        .map(|(_, a)| a)
        .ok_or_else(|| "Aucun fichier Windows dans la dernière release".to_string())?;

    Ok(Release {
        tag: rel.tag_name,
        asset_name: asset.name.clone(),
        asset_url: asset.browser_download_url.clone(),
        size: asset.size,
    })
}

/// Higher is better. None = not usable on Windows x64.
fn asset_score(name: &str) -> Option<u8> {
    let n = name.to_lowercase();
    let foreign = [
        "linux", "macos", "darwin", "-mac", "_mac", "osx", "aarch64", "arm64", "android", "ios",
    ];
    if foreign.iter().any(|f| n.contains(f)) {
        return None;
    }
    let windowsish = n.contains("win") || n.contains("x64") || n.contains("x86_64");
    let installerish = n.contains("setup") || n.contains("install");
    if n.ends_with(".exe") {
        Some(if installerish { 6 } else { 4 })
    } else if n.ends_with(".msi") {
        Some(5)
    } else if n.ends_with(".zip") {
        Some(if windowsish { 3 } else { 1 })
    } else {
        None
    }
}

/// Compares two tags such as "v1.2.10" and "1.2.9". True if `latest` is newer.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    fn nums(t: &str) -> Vec<u64> {
        t.trim_start_matches(|c: char| !c.is_ascii_digit())
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect()
    }
    let (a, b) = (nums(latest), nums(installed));
    if a.is_empty() || b.is_empty() {
        return latest != installed;
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
        assert!(!is_newer("1.0", "1.1"));
        assert!(is_newer("nightly-b", "nightly-a"));
    }

    #[test]
    fn asset_choice() {
        assert!(asset_score("App-Setup-1.0.exe") > asset_score("App.msi"));
        assert!(asset_score("app.msi") > asset_score("app-win64.zip"));
        assert_eq!(asset_score("app-linux.zip"), None);
        assert_eq!(asset_score("app.dmg"), None);
        assert_eq!(asset_score("checksums.txt"), None);
    }
}
