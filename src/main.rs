#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod github;
mod icons;
mod install;
mod store;

use eframe::egui::{self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, RichText, Stroke, Vec2};
use github::{Release, Repo};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use store::Kind;

const ICON_RGBA: &[u8] = include_bytes!("../assets/icon-128.rgba");
const SELF_REPO: &str = "Les-Doubles-A/Ambyst-App";

const GAP: f32 = 12.0;
const PAD: f32 = 16.0;
const CARD_H: f32 = 210.0;

const ACCENT: Color32 = Color32::from_rgb(109, 79, 208);
const BG: Color32 = Color32::from_rgb(246, 246, 247);
const CARD: Color32 = Color32::WHITE;
const BORDER: Color32 = Color32::from_rgb(226, 226, 230);
const TEXT: Color32 = Color32::from_rgb(28, 28, 32);
const MUTED: Color32 = Color32::from_rgb(112, 112, 122);
const OK: Color32 = Color32::from_rgb(34, 128, 74);
const OK_BG: Color32 = Color32::from_rgb(232, 244, 236);
const WARN: Color32 = Color32::from_rgb(176, 96, 0);
const ERR: Color32 = Color32::from_rgb(190, 40, 40);
const TILE_COLORS: [Color32; 6] = [
    Color32::from_rgb(109, 79, 208),
    Color32::from_rgb(40, 110, 180),
    Color32::from_rgb(30, 130, 110),
    Color32::from_rgb(190, 90, 50),
    Color32::from_rgb(160, 60, 110),
    Color32::from_rgb(80, 90, 110),
];

// ---------------------------------------------------------------- model

enum Msg {
    Catalog(Result<Vec<(Repo, bool)>, String>),
    Release(String, Result<Option<Release>, String>),
    Icon(String, Vec<u8>),
    Progress(String, f32),
    Working(String),
    Installed(String, String, Result<install::Installed, String>),
    Uninstalled(String, Result<(), String>),
    Quit,
}

#[derive(Clone)]
enum Busy {
    Checking,
    Downloading(f32),
    Working,
}

#[derive(Clone)]
enum Remover {
    Record(install::WinApp),
    Portable,
}

#[derive(Clone)]
struct InstalledInfo {
    version: String,
    exe: Option<PathBuf>,
    remover: Option<Remover>,
}

struct Item {
    repo: Repo,
    extra: bool,
    release: Option<Result<Option<Release>, String>>,
    icon: Option<egui::TextureHandle>,
    installed: Option<InstalledInfo>,
    busy: Option<Busy>,
    error: Option<String>,
}

impl Item {
    fn key(&self) -> String {
        self.repo.full_name.to_lowercase()
    }

    fn release(&self) -> Option<&Release> {
        self.release.as_ref().and_then(|r| r.as_ref().ok()).and_then(|r| r.as_ref())
    }

    fn has_update(&self) -> bool {
        match (self.release(), &self.installed) {
            (Some(r), Some(i)) => r.windows.is_some() && !i.version.is_empty() && github::is_newer(&r.tag, &i.version),
            _ => false,
        }
    }

    /// Hidden: catalog repositories without any release (sites, libraries...).
    fn hidden(&self) -> bool {
        !self.extra && matches!(self.release, Some(Ok(None)))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    All,
    Installed,
    Updates,
}

struct App {
    data: store::Data,
    items: Vec<Item>,
    catalog: Option<Result<(), String>>,
    tab: Tab,
    search: String,
    confirm_uninstall: Option<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
    logo: egui::TextureHandle,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_style(&cc.egui_ctx);
        let (tx, rx) = channel();
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([128, 128], ICON_RGBA),
            egui::TextureOptions::LINEAR,
        );
        let mut app = Self {
            data: store::load(),
            items: Vec::new(),
            catalog: None,
            tab: Tab::All,
            search: String::new(),
            confirm_uninstall: None,
            tx,
            rx,
            ctx: cc.egui_ctx.clone(),
            logo,
        };
        app.refresh();
        app
    }

    fn item_mut(&mut self, key: &str) -> Option<&mut Item> {
        self.items.iter_mut().find(|i| i.key() == key)
    }

    /// Reloads the catalog, then every release and missing icon.
    fn refresh(&mut self) {
        if matches!(self.catalog, None) && !self.items.is_empty() {
            return;
        }
        self.catalog = None;
        let extras = self.data.extra_repos.clone();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let res = github::list_repos(github::OWNER).map(|repos| {
                let mut out: Vec<(Repo, bool)> = repos.into_iter().map(|r| (r, false)).collect();
                for extra in extras {
                    if out.iter().any(|(r, _)| r.full_name.eq_ignore_ascii_case(&extra)) {
                        continue;
                    }
                    let repo = github::repo_info(&extra).unwrap_or_else(|_| Repo {
                        name: extra.rsplit('/').next().unwrap_or(&extra).to_string(),
                        full_name: extra.clone(),
                        description: String::new(),
                        default_branch: "HEAD".into(),
                    });
                    out.push((repo, true));
                }
                out
            });
            github::save_cache();
            let _ = tx.send(Msg::Catalog(res));
            ctx.request_repaint();
        });
    }

    fn on_catalog(&mut self, repos: Vec<(Repo, bool)>) {
        let mut old: Vec<Item> = std::mem::take(&mut self.items);
        for (repo, extra) in repos {
            let key = repo.full_name.to_lowercase();
            let prev = old.iter().position(|i| i.key() == key).map(|p| old.swap_remove(p));
            let icon = prev.as_ref().and_then(|p| p.icon.clone()).or_else(|| {
                icons::cached(&repo.full_name).map(|rgba| self.texture(&key, &rgba))
            });
            self.items.push(Item {
                release: prev.as_ref().and_then(|p| p.release.clone()),
                installed: prev.as_ref().and_then(|p| p.installed.clone()),
                busy: Some(Busy::Checking),
                error: None,
                icon,
                repo,
                extra,
            });
        }
        self.items.sort_by_key(|i| i.repo.name.to_lowercase());
        self.refresh_installed();

        let mut jobs: VecDeque<Job> = VecDeque::new();
        for item in &self.items {
            jobs.push_back(Job::Release(item.repo.full_name.clone()));
        }
        for item in &self.items {
            if item.icon.is_none() && !icons::recently_missing(&item.repo.full_name) {
                jobs.push_back(Job::Icon(item.repo.clone()));
            }
        }
        run_jobs(jobs, self.tx.clone(), self.ctx.clone());
    }

    fn texture(&self, key: &str, rgba: &[u8]) -> egui::TextureHandle {
        let s = icons::SIZE as usize;
        self.ctx.load_texture(
            format!("icon-{key}"),
            egui::ColorImage::from_rgba_unmultiplied([s, s], rgba),
            egui::TextureOptions::LINEAR,
        )
    }

    /// Matches every item with what is installed on this PC.
    fn refresh_installed(&mut self) {
        let records = install::scan_installed();
        for item in &mut self.items {
            let mut names = vec![item.repo.name.clone()];
            if let Some(a) = item.release().and_then(|r| r.windows.as_ref()) {
                names.push(github::asset_product(&a.name));
            }
            let tracked = self.data.installs.get(&item.key());
            item.installed = if let Some(rec) = install::find_record(&records, &names) {
                let version = if rec.version.is_empty() {
                    tracked.map(|t| t.tag.clone()).unwrap_or_default()
                } else {
                    rec.version.clone()
                };
                Some(InstalledInfo {
                    version,
                    exe: rec.launch_target(&item.repo.name),
                    remover: rec.uninstall.as_ref().map(|_| Remover::Record(rec.clone())),
                })
            } else {
                tracked.map(|t| InstalledInfo {
                    version: t.tag.clone(),
                    exe: t.exe_path.clone().filter(|p| p.exists()),
                    remover: (t.kind == Kind::Portable).then_some(Remover::Portable),
                })
            };
        }
    }

    fn check(&mut self, key: &str) {
        let Some(item) = self.item_mut(key) else { return };
        if item.busy.is_some() {
            return;
        }
        item.busy = Some(Busy::Checking);
        item.error = None;
        let full = item.repo.full_name.clone();
        run_jobs(VecDeque::from([Job::Release(full)]), self.tx.clone(), self.ctx.clone());
    }

    fn install(&mut self, key: &str) {
        let Some(item) = self.items.iter_mut().find(|i| i.key() == key) else { return };
        let Some(release) = item.release().cloned() else { return };
        let Some(asset) = release.windows.clone() else { return };
        if item.busy.is_some() {
            return;
        }
        item.busy = Some(Busy::Downloading(0.0));
        item.error = None;
        let name = item.repo.name.clone();
        let is_self = item.repo.full_name.eq_ignore_ascii_case(SELF_REPO);
        let (tx, ctx, key) = (self.tx.clone(), self.ctx.clone(), key.to_string());
        std::thread::spawn(move || {
            let mut last = -1.0f32;
            let file = install::download(&asset, |p| {
                if p - last >= 0.01 || p >= 1.0 {
                    last = p;
                    let _ = tx.send(Msg::Progress(key.clone(), p));
                    ctx.request_repaint();
                }
            });
            let _ = tx.send(Msg::Working(key.clone()));
            ctx.request_repaint();

            if is_self && asset.name.to_lowercase().ends_with(".exe") {
                // The setup replaces this program: start it and close.
                match file.and_then(|f| install::start_detached(&f)) {
                    Ok(()) => {
                        let _ = tx.send(Msg::Quit);
                    }
                    Err(e) => {
                        let _ = tx.send(Msg::Installed(key, release.tag, Err(e)));
                    }
                }
                ctx.request_repaint();
                return;
            }

            let res = file.and_then(|f| {
                let r = install::install(&name, &f);
                let _ = std::fs::remove_file(&f);
                r
            });
            let _ = tx.send(Msg::Installed(key, release.tag, res));
            ctx.request_repaint();
        });
    }

    fn uninstall(&mut self, key: &str) {
        let Some(item) = self.items.iter_mut().find(|i| i.key() == key) else { return };
        let Some(remover) = item.installed.as_ref().and_then(|i| i.remover.clone()) else { return };
        item.busy = Some(Busy::Working);
        item.error = None;
        let name = item.repo.name.clone();
        let (tx, ctx, key) = (self.tx.clone(), self.ctx.clone(), key.to_string());
        std::thread::spawn(move || {
            let res = match remover {
                Remover::Record(rec) => rec.uninstall(),
                Remover::Portable => {
                    install::remove_portable(&name);
                    Ok(())
                }
            };
            let _ = tx.send(Msg::Uninstalled(key, res));
            ctx.request_repaint();
        });
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Catalog(Ok(repos)) => {
                    self.catalog = Some(Ok(()));
                    self.on_catalog(repos);
                }
                Msg::Catalog(Err(e)) => self.catalog = Some(Err(e)),
                Msg::Release(key, res) => {
                    let had_asset = self.item_mut(&key).and_then(|i| i.release().map(|r| r.windows.is_some()));
                    if let Some(item) = self.item_mut(&key) {
                        item.busy = None;
                        item.release = Some(res);
                    }
                    let has_asset = self.item_mut(&key).and_then(|i| i.release().map(|r| r.windows.is_some()));
                    if had_asset != has_asset {
                        // The asset name helps recognise the installed app.
                        self.refresh_installed();
                    }
                    if self.items.iter().all(|i| !matches!(i.busy, Some(Busy::Checking))) {
                        github::save_cache();
                    }
                }
                Msg::Icon(key, rgba) => {
                    let tex = self.texture(&key, &rgba);
                    if let Some(item) = self.item_mut(&key) {
                        item.icon = Some(tex);
                    }
                }
                Msg::Progress(key, p) => {
                    if let Some(item) = self.item_mut(&key) {
                        item.busy = Some(Busy::Downloading(p));
                    }
                }
                Msg::Working(key) => {
                    if let Some(item) = self.item_mut(&key) {
                        item.busy = Some(Busy::Working);
                    }
                }
                Msg::Installed(key, tag, res) => {
                    let ok = match res {
                        Ok(done) => {
                            self.data.installs.insert(
                                key.clone(),
                                store::Install { tag, kind: done.kind, exe_path: done.exe_path },
                            );
                            store::save(&self.data);
                            None
                        }
                        Err(e) => Some(e),
                    };
                    self.refresh_installed();
                    if let Some(item) = self.item_mut(&key) {
                        item.busy = None;
                        item.error = ok;
                    }
                }
                Msg::Uninstalled(key, res) => {
                    if res.is_ok() {
                        self.data.installs.remove(&key);
                        store::save(&self.data);
                    }
                    self.refresh_installed();
                    if let Some(item) = self.item_mut(&key) {
                        item.busy = None;
                        item.error = res.err();
                        // Some uninstallers return before they finish.
                        if item.error.is_none() && item.installed.is_some() {
                            item.error = Some("La désinstallation se termine, cliquez sur Actualiser".into());
                        }
                    }
                }
                Msg::Quit => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }

    fn add_extra(&mut self, repo: String) {
        if !self.data.extra_repos.iter().any(|r| r.eq_ignore_ascii_case(&repo)) {
            self.data.extra_repos.push(repo);
            store::save(&self.data);
        }
        self.search.clear();
        self.refresh();
    }

    fn remove_extra(&mut self, key: &str) {
        self.data.extra_repos.retain(|r| r.to_lowercase() != key);
        store::save(&self.data);
        self.items.retain(|i| i.key() != key);
    }

    fn visible(&self) -> Vec<usize> {
        let q = self.search.trim().to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| !i.hidden())
            .filter(|(_, i)| match self.tab {
                Tab::All => true,
                Tab::Installed => i.installed.is_some(),
                Tab::Updates => i.has_update(),
            })
            .filter(|(_, i)| {
                q.is_empty()
                    || i.repo.name.to_lowercase().contains(&q)
                    || i.repo.description.to_lowercase().contains(&q)
            })
            .map(|(n, _)| n)
            .collect()
    }
}

// ---------------------------------------------------------------- background jobs

enum Job {
    Release(String),
    Icon(Repo),
}

fn run_jobs(jobs: VecDeque<Job>, tx: Sender<Msg>, ctx: egui::Context) {
    let queue = Arc::new(Mutex::new(jobs));
    for _ in 0..6 {
        let (queue, tx, ctx) = (queue.clone(), tx.clone(), ctx.clone());
        std::thread::spawn(move || loop {
            let job = queue.lock().ok().and_then(|mut q| q.pop_front());
            let Some(job) = job else { break };
            match job {
                Job::Release(full) => {
                    let res = github::latest_release(&full);
                    let _ = tx.send(Msg::Release(full.to_lowercase(), res));
                }
                Job::Icon(repo) => {
                    if let Some(rgba) = icons::fetch(&repo.full_name, &repo.name, &repo.default_branch) {
                        let _ = tx.send(Msg::Icon(repo.full_name.to_lowercase(), rgba));
                    }
                }
            }
            ctx.request_repaint();
        });
    }
}

// ---------------------------------------------------------------- UI

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_messages();

        egui::Panel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(CARD)
                    .inner_margin(Margin::symmetric(20, 14))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(&self.logo).fit_to_exact_size(Vec2::splat(28.0)));
                    ui.label(RichText::new("Ambyst-App").size(20.0).color(TEXT));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let loading = self.catalog.is_none()
                            || self.items.iter().any(|i| i.busy.is_some());
                        if ui.add_enabled(!loading, secondary("Actualiser")).clicked() {
                            self.refresh();
                        }
                        if loading {
                            ui.spinner();
                        }
                        let todo: Vec<String> = self
                            .items
                            .iter()
                            .filter(|i| i.has_update() && i.busy.is_none() && !i.repo.full_name.eq_ignore_ascii_case(SELF_REPO))
                            .map(Item::key)
                            .collect();
                        if !todo.is_empty()
                            && ui.add(primary(&format!("Tout mettre à jour ({})", todo.len()))).clicked()
                        {
                            for key in todo {
                                self.install(&key);
                            }
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(20)))
            .show(ui, |ui| {
                self.toolbar(ui);
                ui.add_space(14.0);

                match &self.catalog {
                    None if self.items.is_empty() => {
                        ui.add_space(60.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.add_space(6.0);
                            ui.label(RichText::new("Chargement des apps...").color(MUTED));
                        });
                        return;
                    }
                    Some(Err(e)) if self.items.is_empty() => {
                        let e = e.clone();
                        ui.add_space(60.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("Impossible de charger les apps").size(16.0).color(TEXT));
                            ui.label(RichText::new(e).color(ERR));
                            ui.add_space(8.0);
                            if ui.add(secondary("Réessayer")).clicked() {
                                self.refresh();
                            }
                        });
                        return;
                    }
                    _ => {}
                }

                let visible = self.visible();
                if visible.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        let text = match self.tab {
                            _ if !self.search.trim().is_empty() => "Aucune app ne correspond à la recherche",
                            Tab::Installed => "Aucune app installée",
                            Tab::Updates => "Tout est à jour",
                            Tab::All => "Aucune app",
                        };
                        ui.label(RichText::new(text).size(15.0).color(MUTED));
                        if let Some(repo) = store::parse_repo(&self.search).filter(|_| self.search.contains('/')) {
                            ui.add_space(10.0);
                            if ui.add(primary(&format!("Ajouter {repo}"))).clicked() {
                                self.add_extra(repo);
                            }
                        }
                    });
                    return;
                }

                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let width = ui.available_width() - 2.0;
                    let cols = if width >= 900.0 { 3 } else if width >= 560.0 { 2 } else { 1 };
                    let card_w = (width - GAP * (cols - 1) as f32) / cols as f32;
                    for chunk in visible.chunks(cols) {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = GAP;
                            for &idx in chunk {
                                if idx < self.items.len() {
                                    self.card(ui, idx, card_w);
                                }
                            }
                        });
                        ui.add_space(GAP);
                    }
                });
            });
    }
}

impl App {
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let shown = |f: &dyn Fn(&Item) -> bool| self.items.iter().filter(|i| !i.hidden() && f(i)).count();
        let counts = [
            (Tab::All, "Toutes", shown(&|_| true)),
            (Tab::Installed, "Installées", shown(&|i| i.installed.is_some())),
            (Tab::Updates, "Mises à jour", shown(&|i| i.has_update())),
        ];
        ui.horizontal(|ui| {
            for (tab, label, n) in counts {
                let selected = self.tab == tab;
                let text = RichText::new(format!("{label}  {n}")).color(if selected { Color32::WHITE } else { TEXT });
                let btn = egui::Button::new(text)
                    .fill(if selected { TEXT } else { CARD })
                    .stroke(if selected { Stroke::NONE } else { Stroke::new(1.0, BORDER) })
                    .corner_radius(CornerRadius::same(16))
                    .min_size(Vec2::new(0.0, 32.0));
                if ui.add(btn).clicked() {
                    self.tab = tab;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Rechercher, ou coller propriétaire/dépôt")
                        .desired_width(ui.available_width().min(300.0))
                        .margin(Margin::symmetric(10, 7)),
                );
            });
        });
    }

    fn card(&mut self, ui: &mut egui::Ui, idx: usize, width: f32) {
        let key = self.items[idx].key();
        let mut action: Option<&'static str> = None;
        let inner = Vec2::new(width - 2.0 * PAD - 2.0, CARD_H - 2.0 * PAD - 2.0);

        egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(PAD as i8))
            .show(ui, |ui| {
                ui.set_min_size(inner);
                ui.set_max_size(inner);
                let item = &self.items[idx];
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        app_icon(ui, item, 56.0);
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.add_space(6.0);
                            ui.add(egui::Label::new(RichText::new(&item.repo.name).size(17.0).color(TEXT)).truncate());
                            let (text, color) = status_line(item);
                            ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).truncate());
                        });
                    });
                    ui.add_space(10.0);

                    let desc = item.repo.description.split_whitespace().collect::<Vec<_>>().join(" ");
                    let mut job = egui::text::LayoutJob::simple(desc, FontId::proportional(13.0), MUTED, inner.x);
                    job.wrap.max_rows = 2;
                    job.wrap.break_anywhere = false;
                    job.wrap.overflow_character = Some('…');
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(inner.x, 36.0), egui::Sense::hover());
                    let galley = ui.painter().layout_job(job);
                    ui.painter().galley(rect.min, galley, MUTED);

                    if let Some(err) = &item.error {
                        ui.add(egui::Label::new(RichText::new(err).size(12.5).color(ERR)).truncate());
                    }

                    ui.with_layout(Layout::bottom_up(Align::Max), |ui| {
                        ui.allocate_ui_with_layout(
                            Vec2::new(inner.x, 34.0),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                action = row_buttons(ui, item, self.confirm_uninstall.as_deref() == Some(key.as_str()));
                            },
                        );
                    });
                });
            });

        match action {
            Some("install") => self.install(&key),
            Some("retry") => self.check(&key),
            Some("open") => {
                let exe = self.items[idx].installed.as_ref().and_then(|i| i.exe.clone());
                if let Some(exe) = exe {
                    let err = install::launch(&exe).err();
                    self.items[idx].error = err;
                }
            }
            Some("page") => {
                let item = &self.items[idx];
                let url = item
                    .release()
                    .map(|r| r.page.clone())
                    .unwrap_or_else(|| format!("https://github.com/{}", item.repo.full_name));
                install::open_url(&url);
            }
            Some("ask_uninstall") => self.confirm_uninstall = Some(key),
            Some("cancel") => self.confirm_uninstall = None,
            Some("uninstall") => {
                self.confirm_uninstall = None;
                self.uninstall(&key);
            }
            Some("remove") => self.remove_extra(&key),
            _ => {}
        }
    }
}

/// Right-hand buttons of a row (laid out right to left). Returns the clicked action.
fn row_buttons(ui: &mut egui::Ui, item: &Item, confirming: bool) -> Option<&'static str> {
    let mut action = None;
    let mut click = |clicked: bool, a: &'static str| {
        if clicked {
            action = Some(a);
        }
    };

    if confirming {
        click(ui.add(danger("Désinstaller")).clicked(), "uninstall");
        click(ui.add(secondary("Annuler")).clicked(), "cancel");
        return action;
    }

    match &item.busy {
        Some(Busy::Downloading(p)) => {
            ui.add(egui::ProgressBar::new(*p).desired_width(150.0).fill(ACCENT).show_percentage());
            return None;
        }
        Some(_) => {
            ui.spinner();
            return None;
        }
        None => {}
    }

    let release = item.release();
    let windows = release.is_some_and(|r| r.windows.is_some());
    match (&item.installed, release) {
        (Some(_), _) if item.has_update() => click(ui.add(primary("Mettre à jour")).clicked(), "install"),
        (Some(_), _) => badge(ui, "Installé", OK_BG, OK),
        (None, Some(_)) if windows => click(ui.add(primary("Installer")).clicked(), "install"),
        (None, Some(r)) if r.android.is_some() => {
            click(ui.add(secondary("Voir l'APK")).clicked(), "page")
        }
        (None, Some(_)) => click(ui.add(secondary("Voir sur GitHub")).clicked(), "page"),
        (None, None) => {
            if matches!(item.release, Some(Err(_))) {
                click(ui.add(secondary("Réessayer")).clicked(), "retry");
            }
        }
    }

    if let Some(info) = &item.installed {
        if info.exe.is_some() {
            click(ui.add(secondary("Ouvrir")).clicked(), "open");
        }
        if info.remover.is_some() {
            click(ui.add(ghost("Désinstaller")).clicked(), "ask_uninstall");
        }
    } else if item.extra {
        click(ui.add(ghost("Retirer")).on_hover_text("Retirer de la liste").clicked(), "remove");
    }
    action
}

fn status_line(item: &Item) -> (String, Color32) {
    match &item.busy {
        Some(Busy::Checking) if item.release.is_none() => return ("Vérification...".into(), MUTED),
        Some(Busy::Downloading(_)) => return ("Téléchargement...".into(), MUTED),
        Some(Busy::Working) => return ("Opération en cours, suivez la fenêtre qui s'ouvre...".into(), MUTED),
        _ => {}
    }
    let installed = item.installed.as_ref().map(|i| i.version.as_str());
    match (&item.release, installed) {
        (Some(Err(e)), Some(v)) => (format!("Installée  ·  {}  ·  {e}", vlabel(v)), MUTED),
        (Some(Err(e)), None) => (e.clone(), ERR),
        (Some(Ok(Some(r))), Some(v)) if item.has_update() => {
            (format!("Mise à jour disponible  ·  {}  >  {}", vlabel(v), vlabel(&r.tag)), WARN)
        }
        (Some(Ok(Some(_))) | None, Some(v)) => (format!("Installée  ·  {}", vlabel(v)), OK),
        (Some(Ok(Some(r))), None) if r.windows.is_some() => (vlabel(&r.tag), MUTED),
        (Some(Ok(Some(r))), None) if r.android.is_some() => {
            (format!("Application Android  ·  {}", vlabel(&r.tag)), MUTED)
        }
        (Some(Ok(Some(r))), None) => (format!("{}  ·  aucun fichier Windows", vlabel(&r.tag)), MUTED),
        (Some(Ok(None)), _) => ("Aucune release publiée".into(), MUTED),
        (None, None) => ("Vérification...".into(), MUTED),
    }
}

fn vlabel(v: &str) -> String {
    if v.is_empty() {
        "version inconnue".into()
    } else if v.starts_with(|c: char| c.is_ascii_digit()) {
        format!("v{v}")
    } else {
        v.to_string()
    }
}

fn app_icon(ui: &mut egui::Ui, item: &Item, side: f32) {
    let size = Vec2::splat(side);
    if let Some(tex) = &item.icon {
        ui.add(egui::Image::new(tex).fit_to_exact_size(size).corner_radius(CornerRadius::same(12)));
        return;
    }
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let hash = item.repo.name.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    ui.painter().rect_filled(rect, CornerRadius::same(12), TILE_COLORS[hash % TILE_COLORS.len()]);
    let letter = item.repo.name.chars().next().unwrap_or('?').to_uppercase().to_string();
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, letter, FontId::proportional(side * 0.45), Color32::WHITE);
}

/// A non-clickable, button-sized label.
fn badge(ui: &mut egui::Ui, text: &str, fill: Color32, color: Color32) {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let size = Vec2::new(galley.size().x + 28.0, 32.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
}

fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE))
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(0.0, 32.0))
}

fn secondary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(TEXT))
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(0.0, 32.0))
}

fn ghost(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(MUTED))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, 32.0))
}

fn danger(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE))
        .fill(ERR)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(0.0, 32.0))
}

fn apply_style(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Light);
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = BG;
    visuals.window_fill = CARD;
    visuals.extreme_bg_color = CARD;
    visuals.selection.bg_fill = ACCENT.linear_multiply(0.25);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(6);
    }
    visuals.window_shadow = egui::Shadow::NONE;
    visuals.popup_shadow = egui::Shadow::NONE;
    ctx.set_visuals_of(egui::Theme::Light, visuals);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 5.0);
        s.spacing.button_padding = Vec2::new(14.0, 6.0);
    });
}

fn main() -> eframe::Result {
    let icon = egui::IconData { rgba: ICON_RGBA.to_vec(), width: 128, height: 128 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ambyst-App")
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([640.0, 420.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native("Ambyst-App", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
