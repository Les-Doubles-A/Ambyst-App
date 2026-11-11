use crate::github::Asset;
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

// ---------------------------------------------------------------- download

/// Downloads a release asset, reporting progress in [0, 1].
pub fn download(asset: &Asset, mut progress: impl FnMut(f32)) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("ambyst-app");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file_name = Path::new(&asset.name).file_name().ok_or("Nom de fichier invalide")?;
    let path = dir.join(file_name);

    let resp = crate::github::get(&asset.url)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|_| "Téléchargement impossible".to_string())?;
    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;

    let total = asset.size.max(1) as f32;
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

// ---------------------------------------------------------------- install

/// Installs a downloaded asset. Blocks until a setup program exits.
pub fn install(app_name: &str, file: &Path) -> Result<Installed, String> {
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
        return Ok(Installed { kind: Kind::Installer, exe_path: None });
    }

    if lower.ends_with(".exe") && crate::github::is_setup_name(&lower) {
        run_and_wait(file, "")?;
        return Ok(Installed { kind: Kind::Installer, exe_path: None });
    }

    let target = store::apps_dir().join(safe_dir_name(app_name));
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .map_err(|_| "Fermez l'app avant de la mettre à jour".to_string())?;
    }
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;

    if lower.ends_with(".zip") {
        let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(f).map_err(|_| "Archive ZIP invalide".to_string())?;
        zip.extract(&target).map_err(|_| "Extraction impossible".to_string())?;
        let exe = best_exe(&target, app_name, 3);
        return Ok(Installed { kind: Kind::Portable, exe_path: exe });
    }

    let dest = target.join(file.file_name().ok_or("Nom de fichier invalide")?);
    std::fs::copy(file, &dest).map_err(|e| e.to_string())?;
    Ok(Installed { kind: Kind::Portable, exe_path: Some(dest) })
}

/// Starts a setup program without waiting (used to update Ambyst-App itself).
pub fn start_detached(file: &Path) -> Result<(), String> {
    let mut cmd = Command::new("powershell");
    let path = ps_quote(&file.to_string_lossy());
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &format!("Start-Process -FilePath {path}")]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err("Installation annulée".into()) }
}

fn check_exit(code: Option<i32>) -> Result<(), String> {
    match code {
        Some(0) | Some(3010) => Ok(()),
        Some(1602) | Some(1223) => Err("Opération annulée".into()),
        Some(c) => Err(format!("Le programme a échoué (code {c})")),
        None => Err("Le programme a été interrompu".into()),
    }
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Runs a program through the shell so that UAC elevation prompts work.
fn run_and_wait(file: &Path, args: &str) -> Result<(), String> {
    let mut script = format!("$p = Start-Process -FilePath {} -PassThru -Wait", ps_quote(&file.to_string_lossy()));
    if !args.trim().is_empty() {
        script.push_str(&format!(" -ArgumentList {}", ps_quote(args.trim())));
    }
    script.push_str("; exit $p.ExitCode");
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.code() == Some(1) {
        // Start-Process failed, most often because UAC was refused.
        return Err("Opération annulée".into());
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

pub fn open_url(url: &str) {
    let _ = Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
}

pub fn remove_portable(app_name: &str) {
    let _ = std::fs::remove_dir_all(store::apps_dir().join(safe_dir_name(app_name)));
}

fn safe_dir_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '_' })
        .collect()
}

pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

// ---------------------------------------------------------------- Windows records

/// An entry of "Apps and features".
#[derive(Clone, Debug)]
pub struct WinApp {
    pub display_name: String,
    pub version: String,
    pub exe: Option<PathBuf>,
    pub install_dir: Option<PathBuf>,
    pub uninstall: Option<String>,
}

/// Reads the uninstall entries of the registry (current user and machine).
#[cfg(windows)]
pub fn scan_installed() -> Vec<WinApp> {
    use winreg::{HKCU, HKLM};
    const PATHS: [&str; 2] = [
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
        r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    let mut out = Vec::new();
    for root in [HKCU, HKLM] {
        for path in PATHS {
            let Ok(key) = root.open_subkey(path) else { continue };
            for sub in key.enum_keys().flatten() {
                let Ok(k) = key.open_subkey(&sub) else { continue };
                let Ok(display_name) = k.get_value::<String, _>("DisplayName") else { continue };
                let icon = k.get_value::<String, _>("DisplayIcon").ok();
                let exe = icon
                    .map(|s| {
                        let s = s.trim().trim_matches('"');
                        let s = match s.rsplit_once(',') {
                            Some((p, idx)) if idx.trim().parse::<i32>().is_ok() => p,
                            _ => s,
                        };
                        PathBuf::from(s.trim().trim_matches('"'))
                    })
                    .filter(|p| {
                        p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
                            && !norm(&p.file_stem().unwrap_or_default().to_string_lossy()).starts_with("unins")
                    });
                out.push(WinApp {
                    display_name,
                    version: k.get_value::<String, _>("DisplayVersion").unwrap_or_default(),
                    exe,
                    install_dir: k
                        .get_value::<String, _>("InstallLocation")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                        .map(|s| PathBuf::from(s.trim().trim_matches('"'))),
                    uninstall: k.get_value::<String, _>("UninstallString").ok(),
                });
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn scan_installed() -> Vec<WinApp> {
    Vec::new()
}

/// "GoldenHue version 2.2.0" -> "goldenhue"
fn display_key(display_name: &str) -> String {
    let lower = display_name.to_lowercase();
    let base = match lower.find(" version ") {
        Some(i) => &display_name[..i],
        None => display_name,
    };
    norm(base)
}

/// Finds the Windows record of an app, by exact (normalized) name.
pub fn find_record<'a>(records: &'a [WinApp], names: &[String]) -> Option<&'a WinApp> {
    let wanted: Vec<String> = names.iter().map(|n| norm(n)).filter(|n| !n.is_empty()).collect();
    records.iter().find(|r| wanted.contains(&display_key(&r.display_name)))
}

impl WinApp {
    /// The executable to open: the registered icon, or the best match in the install folder.
    pub fn launch_target(&self, app_name: &str) -> Option<PathBuf> {
        if let Some(exe) = self.exe.as_ref().filter(|p| p.exists()) {
            return Some(exe.clone());
        }
        let dir = self
            .install_dir
            .clone()
            .or_else(|| self.uninstall.as_deref().and_then(split_command).and_then(|(exe, _)| exe.parent().map(Path::to_path_buf)))?;
        best_exe(&dir, app_name, 2)
    }

    pub fn uninstall(&self) -> Result<(), String> {
        let cmd = self.uninstall.as_deref().ok_or("Aucun désinstalleur trouvé")?;
        let (exe, args) = split_command(cmd).ok_or("Aucun désinstalleur trouvé")?;
        run_and_wait(&exe, &args)
    }
}

/// Splits `"C:\x\unins000.exe" /SILENT` or `MsiExec.exe /X{...}` into program and arguments.
fn split_command(cmd: &str) -> Option<(PathBuf, String)> {
    let cmd = cmd.trim();
    if let Some(rest) = cmd.strip_prefix('"') {
        let (exe, args) = rest.split_once('"')?;
        return Some((PathBuf::from(exe), args.trim().to_string()));
    }
    let lower = cmd.to_lowercase();
    match lower.find(".exe") {
        Some(i) => Some((PathBuf::from(&cmd[..i + 4]), cmd[i + 4..].trim().to_string())),
        None => Some((PathBuf::from(cmd), String::new())),
    }
}

/// Picks the executable whose name best matches the app name.
pub fn best_exe(dir: &Path, app_name: &str, depth: u32) -> Option<PathBuf> {
    let wanted = norm(app_name);
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
            } else if !n.is_empty() && (n.contains(&wanted) || wanted.contains(&n)) {
                2
            } else {
                1
            };
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
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names() {
        assert_eq!(display_key("GoldenHue version 2.2.0"), "goldenhue");
        assert_eq!(display_key("SwiftPad"), "swiftpad");
        assert_eq!(display_key("clean-peek version 1.0.0"), "cleanpeek");
    }

    #[test]
    fn commands() {
        let (e, a) = split_command(r#""C:\a b\unins000.exe" /SILENT"#).unwrap();
        assert_eq!(e, PathBuf::from(r"C:\a b\unins000.exe"));
        assert_eq!(a, "/SILENT");
        let (e, a) = split_command("MsiExec.exe /X{ABC}").unwrap();
        assert_eq!(e, PathBuf::from("MsiExec.exe"));
        assert_eq!(a, "/X{ABC}");
    }

    /// Network test: `cargo test -- --ignored`
    #[test]
    #[ignore]
    fn end_to_end_portable_zip() {
        let rel = crate::github::latest_release("BurntSushi/ripgrep").unwrap().unwrap();
        let asset = rel.windows.unwrap();
        assert!(asset.name.ends_with(".zip"), "{}", asset.name);
        let file = download(&asset, |_| {}).unwrap();
        let done = install("ambyst-test-ripgrep", &file).unwrap();
        assert!(done.exe_path.is_some());
        remove_portable("ambyst-test-ripgrep");
        let _ = std::fs::remove_file(file);
    }
}
