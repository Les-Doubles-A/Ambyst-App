use crate::github::{self, Release};
use crate::store::{self, Kind};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Installed {
    pub kind: Kind,
    pub exe_path: Option<PathBuf>,
}

/// Downloads the release asset, reporting progress in [0, 1].
pub fn download(release: &Release, mut progress: impl FnMut(f32)) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("ambyst-app");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file_name = Path::new(&release.asset_name)
        .file_name()
        .ok_or("Nom de fichier invalide")?;
    let path = dir.join(file_name);

    let resp = github::get(&release.asset_url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|_| "Téléchargement impossible".to_string())?;
    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;

    let total = release.size.max(1) as f32;
    let mut done: u64 = 0;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|_| "Téléchargement interrompu".to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress((done as f32 / total).min(1.0));
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(path)
}

/// Installs a downloaded asset. Blocks until a setup program exits.
pub fn install(repo_name: &str, file: &Path) -> Result<Installed, String> {
    let lower = file
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if lower.ends_with(".msi") {
        let status = Command::new("msiexec")
            .arg("/i")
            .arg(file)
            .arg("/passive")
            .status()
            .map_err(|e| e.to_string())?;
        check_exit(status.code())?;
        return Ok(Installed { kind: Kind::Installer, exe_path: find_installed_exe(repo_name) });
    }

    if lower.ends_with(".exe") && (lower.contains("setup") || lower.contains("install")) {
        run_and_wait(file)?;
        return Ok(Installed { kind: Kind::Installer, exe_path: find_installed_exe(repo_name) });
    }

    let target = store::apps_dir().join(safe_dir_name(repo_name));
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .map_err(|_| "Fermez l'app avant de la mettre à jour".to_string())?;
    }
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    if lower.ends_with(".zip") {
        let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(f).map_err(|_| "Archive ZIP invalide".to_string())?;
        zip.extract(&target).map_err(|_| "Extraction impossible".to_string())?;
        let exe = best_exe(&target, repo_name, 3);
        return Ok(Installed { kind: Kind::Portable, exe_path: exe });
    }

    let dest = target.join(file.file_name().ok_or("Nom de fichier invalide")?);
    std::fs::copy(file, &dest).map_err(|e| e.to_string())?;
    Ok(Installed { kind: Kind::Portable, exe_path: Some(dest) })
}

fn check_exit(code: Option<i32>) -> Result<(), String> {
    match code {
        Some(0) | Some(3010) => Ok(()),
        Some(1602) | Some(1223) => Err("Installation annulée".into()),
        Some(c) => Err(format!("L'installeur a échoué (code {c})")),
        None => Err("L'installeur a été interrompu".into()),
    }
}

/// Runs through the shell so that UAC elevation prompts work.
fn run_and_wait(file: &Path) -> Result<(), String> {
    let path = file.to_string_lossy().replace('\'', "''");
    let script = format!(
        "$p = Start-Process -FilePath '{path}' -PassThru -Wait; exit $p.ExitCode"
    );
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.code() == Some(1) {
        // Start-Process failed (often: UAC refused).
        return Err("Installation annulée".into());
    }
    check_exit(status.code())
}

pub fn launch(exe: &Path) -> Result<(), String> {
    let mut cmd = Command::new(exe);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.spawn().map(|_| ()).map_err(|_| "Impossible de lancer l'app".to_string())
}

pub fn remove_portable(repo_name: &str) {
    let _ = std::fs::remove_dir_all(store::apps_dir().join(safe_dir_name(repo_name)));
}

fn safe_dir_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' })
        .collect()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

/// Looks in the usual install locations for a folder named like the app.
pub fn find_installed_exe(repo_name: &str) -> Option<PathBuf> {
    let wanted = norm(repo_name);
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(local) = dirs::data_local_dir() {
        roots.push(local.join("Programs"));
        roots.push(local.clone());
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Ok(p) = std::env::var(var) {
            roots.push(PathBuf::from(p));
        }
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && norm(&entry.file_name().to_string_lossy()) == wanted {
                if let Some(exe) = best_exe(&path, repo_name, 2) {
                    return Some(exe);
                }
            }
        }
    }
    None
}

/// Picks the executable whose name best matches the app name.
fn best_exe(dir: &Path, repo_name: &str, depth: u32) -> Option<PathBuf> {
    let wanted = norm(repo_name);
    let mut found = Vec::new();
    collect_exes(dir, depth, &mut found);
    found
        .into_iter()
        .filter(|p| {
            let n = norm(&p.file_stem().unwrap_or_default().to_string_lossy());
            !(n.starts_with("unins") || n.contains("setup") || n.contains("update") || n.contains("crash"))
        })
        .max_by_key(|p| {
            let n = norm(&p.file_stem().unwrap_or_default().to_string_lossy());
            let score = if n == wanted {
                3
            } else if n.contains(&wanted) || wanted.contains(&n) {
                2
            } else {
                1
            };
            // Prefer shallower files on ties.
            (score, std::cmp::Reverse(p.components().count()))
        })
}

fn collect_exes(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                collect_exes(&path, depth - 1, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    /// Network test: `cargo test -- --ignored`
    #[test]
    #[ignore]
    fn end_to_end_portable_zip() {
        let rel = crate::github::latest_release("BurntSushi/ripgrep").unwrap();
        assert!(rel.asset_name.ends_with(".zip"), "{}", rel.asset_name);
        let file = super::download(&rel, |_| {}).unwrap();
        let done = super::install("ambyst-test-ripgrep", &file).unwrap();
        let exe = done.exe_path.expect("exe found");
        assert!(exe.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case("rg.exe"));
        super::remove_portable("ambyst-test-ripgrep");
        let _ = std::fs::remove_file(file);
    }
}
