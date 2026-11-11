#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod github;
mod install;
mod store;

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Vec2};
use github::Release;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use store::{Entry, Kind};

const ICON_RGBA: &[u8] = include_bytes!("../assets/icon-128.rgba");

const ACCENT: Color32 = Color32::from_rgb(109, 79, 208);
const BG: Color32 = Color32::from_rgb(246, 246, 247);
const CARD: Color32 = Color32::WHITE;
const BORDER: Color32 = Color32::from_rgb(226, 226, 230);
const TEXT: Color32 = Color32::from_rgb(28, 28, 32);
const MUTED: Color32 = Color32::from_rgb(112, 112, 122);
const OK: Color32 = Color32::from_rgb(34, 128, 74);
const WARN: Color32 = Color32::from_rgb(176, 96, 0);
const ERR: Color32 = Color32::from_rgb(190, 40, 40);

enum Msg {
    Checked(String, Result<Release, String>),
    Progress(String, f32),
    Installing(String),
    Done(String, String, Result<install::Installed, String>),
}

#[derive(Clone)]
enum Busy {
    Checking,
    Downloading(f32),
    Installing,
}

#[derive(Default)]
struct State {
    latest: Option<Result<Release, String>>,
    busy: Option<Busy>,
    error: Option<String>,
}

struct App {
    entries: Vec<Entry>,
    states: HashMap<String, State>,
    input: String,
    input_error: Option<String>,
    confirm_remove: Option<String>,
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
        let mut entries = store::load();
        // Installers may move things around: re-locate missing executables once.
        for e in entries.iter_mut() {
            if e.kind == Some(Kind::Installer) && !e.exe_path.as_ref().is_some_and(|p| p.exists()) {
                e.exe_path = install::find_installed_exe(e.name());
            }
        }
        let mut app = Self {
            logo,
            entries,
            states: HashMap::new(),
            input: String::new(),
            input_error: None,
            confirm_remove: None,
            tx,
            rx,
            ctx: cc.egui_ctx.clone(),
        };
        app.check_all();
        app
    }

    fn state(&mut self, key: &str) -> &mut State {
        self.states.entry(key.to_string()).or_default()
    }

    fn check_all(&mut self) {
        let repos: Vec<String> = self.entries.iter().map(|e| e.repo.clone()).collect();
        for repo in repos {
            self.check(&repo);
        }
    }

    fn check(&mut self, repo: &str) {
        let key = repo.to_lowercase();
        let st = self.state(&key);
        if st.busy.is_some() {
            return;
        }
        st.busy = Some(Busy::Checking);
        let (tx, ctx, repo) = (self.tx.clone(), self.ctx.clone(), repo.to_string());
        std::thread::spawn(move || {
            let res = github::latest_release(&repo);
            let _ = tx.send(Msg::Checked(key, res));
            ctx.request_repaint();
        });
    }

    fn install(&mut self, idx: usize) {
        let entry = &self.entries[idx];
        let key = entry.key();
        let name = entry.name().to_string();
        let st = self.states.entry(key.clone()).or_default();
        let Some(Ok(release)) = st.latest.clone() else { return };
        if st.busy.is_some() {
            return;
        }
        st.busy = Some(Busy::Downloading(0.0));
        st.error = None;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut last = -1.0f32;
            let file = install::download(&release, |p| {
                if p - last >= 0.01 || p >= 1.0 {
                    last = p;
                    let _ = tx.send(Msg::Progress(key.clone(), p));
                    ctx.request_repaint();
                }
            });
            let res = file.and_then(|f| {
                let _ = tx.send(Msg::Installing(key.clone()));
                ctx.request_repaint();
                let r = install::install(&name, &f);
                let _ = std::fs::remove_file(&f);
                r
            });
            let _ = tx.send(Msg::Done(key, release.tag, res));
            ctx.request_repaint();
        });
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Checked(key, res) => {
                    let st = self.state(&key);
                    st.busy = None;
                    st.latest = Some(res);
                }
                Msg::Progress(key, p) => self.state(&key).busy = Some(Busy::Downloading(p)),
                Msg::Installing(key) => self.state(&key).busy = Some(Busy::Installing),
                Msg::Done(key, tag, res) => {
                    let st = self.state(&key);
                    st.busy = None;
                    match res {
                        Ok(done) => {
                            if let Some(e) = self.entries.iter_mut().find(|e| e.key() == key) {
                                e.installed_tag = Some(tag);
                                e.kind = Some(done.kind);
                                e.exe_path = done.exe_path;
                            }
                            store::save(&self.entries);
                        }
                        Err(err) => st.error = Some(err),
                    }
                }
            }
        }
    }

    fn add_from_input(&mut self) {
        match store::parse_repo(&self.input) {
            Some(repo) => {
                if self.entries.iter().any(|e| e.repo.eq_ignore_ascii_case(&repo)) {
                    self.input_error = Some("Cette app est déjà dans la liste".into());
                    return;
                }
                self.entries.push(Entry::new(repo.clone()));
                store::save(&self.entries);
                self.input.clear();
                self.input_error = None;
                self.check(&repo);
            }
            None => {
                self.input_error =
                    Some("Format attendu : propriétaire/dépôt ou lien GitHub".into());
            }
        }
    }

    fn remove(&mut self, key: &str) {
        if let Some(pos) = self.entries.iter().position(|e| e.key() == key) {
            let e = self.entries.remove(pos);
            if e.kind == Some(Kind::Portable) {
                install::remove_portable(e.name());
            }
            self.states.remove(key);
            store::save(&self.entries);
        }
    }

    fn updatable(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let st = self.states.get(&e.key());
                matches!(
                    (st.and_then(|s| s.latest.as_ref()), &e.installed_tag),
                    (Some(Ok(r)), Some(t)) if github::is_newer(&r.tag, t)
                ) && st.is_some_and(|s| s.busy.is_none())
            })
            .map(|(i, _)| i)
            .collect()
    }
}

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
                    ui.label(RichText::new("Ambyst-App").size(20.0).strong().color(TEXT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(secondary("Vérifier les mises à jour")).clicked() {
                            self.check_all();
                        }
                        let todo = self.updatable();
                        if !todo.is_empty()
                            && ui
                                .add(primary(&format!("Tout mettre à jour ({})", todo.len())))
                                .clicked()
                        {
                            for i in todo {
                                self.install(i);
                            }
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(Margin::same(20)))
            .show(ui, |ui| {
                self.add_row(ui);
                ui.add_space(16.0);

                if self.entries.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Aucune app pour l'instant").size(16.0).color(TEXT));
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Collez le lien d'un dépôt GitHub ci-dessus pour l'ajouter.",
                            )
                            .color(MUTED),
                        );
                    });
                    return;
                }

                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    for i in 0..self.entries.len() {
                        if i >= self.entries.len() {
                            break;
                        }
                        self.row(ui, i);
                        ui.add_space(8.0);
                    }
                });
            });
    }
}

impl App {
    fn add_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let button_w = 90.0;
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.input)
                    .hint_text("propriétaire/dépôt  ou  https://github.com/propriétaire/dépôt")
                    .desired_width(ui.available_width() - button_w - 8.0)
                    .margin(Margin::symmetric(10, 8)),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let click = ui
                .add(primary("Ajouter").min_size(Vec2::new(button_w, 34.0)))
                .clicked();
            if (enter || click) && !self.input.trim().is_empty() {
                self.add_from_input();
            }
        });
        if let Some(err) = &self.input_error {
            ui.add_space(4.0);
            ui.label(RichText::new(err).color(ERR).size(12.5));
        }
    }

    fn row(&mut self, ui: &mut egui::Ui, idx: usize) {
        let entry = self.entries[idx].clone();
        let key = entry.key();
        let st = self.states.entry(key.clone()).or_default();
        let busy = st.busy.clone();
        let latest = st.latest.clone();
        let error = st.error.clone();

        let mut do_install = false;
        let mut do_check = false;
        let mut do_remove = false;
        let mut launch_err = None;

        egui::Frame::new()
            .fill(CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(entry.name()).size(15.5).strong().color(TEXT));
                        ui.label(RichText::new(&entry.repo).size(12.0).color(MUTED));
                        ui.add_space(2.0);
                        let (text, color) = status_line(&entry, latest.as_ref(), busy.as_ref());
                        ui.label(RichText::new(text).size(12.5).color(color));
                        if let Some(err) = &error {
                            ui.label(RichText::new(err).size(12.5).color(ERR));
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.confirm_remove.as_deref() == Some(key.as_str()) {
                            if ui.add(danger("Retirer")).clicked() {
                                do_remove = true;
                            }
                            if ui.add(secondary("Annuler")).clicked() {
                                self.confirm_remove = None;
                            }
                            return;
                        }

                        if ui
                            .add(ghost("Retirer"))
                            .on_hover_text("Retirer de la liste")
                            .clicked()
                        {
                            self.confirm_remove = Some(key.clone());
                        }

                        match &busy {
                            Some(Busy::Downloading(p)) => {
                                ui.add(
                                    egui::ProgressBar::new(*p)
                                        .desired_width(140.0)
                                        .fill(ACCENT)
                                        .show_percentage(),
                                );
                            }
                            Some(Busy::Installing) | Some(Busy::Checking) => {
                                ui.spinner();
                            }
                            None => {
                                let release = latest.as_ref().and_then(|r| r.as_ref().ok());
                                match (&entry.installed_tag, release) {
                                    (None, Some(_)) => {
                                        if ui.add(primary("Installer")).clicked() {
                                            do_install = true;
                                        }
                                    }
                                    (Some(t), Some(r)) if github::is_newer(&r.tag, t) => {
                                        if ui.add(primary("Mettre à jour")).clicked() {
                                            do_install = true;
                                        }
                                    }
                                    (_, None) => {
                                        if ui.add(secondary("Réessayer")).clicked() {
                                            do_check = true;
                                        }
                                    }
                                    _ => {}
                                }
                                if entry.installed_tag.is_some() {
                                    let exe = entry.exe_path.clone().filter(|p| p.exists());
                                    let open = ui.add_enabled(exe.is_some(), secondary("Ouvrir"));
                                    let open = if exe.is_none() {
                                        open.on_disabled_hover_text(
                                            "Lancez l'app depuis le menu Démarrer",
                                        )
                                    } else {
                                        open
                                    };
                                    if open.clicked() {
                                        if let Some(exe) = exe {
                                            launch_err = install::launch(&exe).err();
                                        }
                                    }
                                }
                            }
                        }
                    });
                });
            });

        if let Some(err) = launch_err {
            self.state(&key).error = Some(err);
        }
        if do_install {
            self.install(idx);
        }
        if do_check {
            self.state(&key).error = None;
            self.check(&entry.repo);
        }
        if do_remove {
            self.confirm_remove = None;
            self.remove(&key);
        }
    }
}

fn status_line(
    entry: &Entry,
    latest: Option<&Result<Release, String>>,
    busy: Option<&Busy>,
) -> (String, Color32) {
    match busy {
        Some(Busy::Checking) => return ("Vérification...".into(), MUTED),
        Some(Busy::Downloading(_)) => return ("Téléchargement...".into(), MUTED),
        Some(Busy::Installing) => return ("Installation en cours...".into(), MUTED),
        None => {}
    }
    match (latest, &entry.installed_tag) {
        (Some(Err(e)), _) => (e.clone(), ERR),
        (Some(Ok(r)), None) => (format!("Non installée  ·  {} disponible", r.tag), MUTED),
        (Some(Ok(r)), Some(t)) if github::is_newer(&r.tag, t) => {
            (format!("Mise à jour disponible  ·  {t}  >  {}", r.tag), WARN)
        }
        (Some(Ok(_)), Some(t)) => (format!("À jour  ·  {t}"), OK),
        (None, Some(t)) => (format!("Installée  ·  {t}"), MUTED),
        (None, None) => ("Non installée".into(), MUTED),
    }
}

fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE))
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(0.0, 30.0))
}

fn secondary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(TEXT))
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(0.0, 30.0))
}

fn ghost(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(MUTED))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE)
        .min_size(Vec2::new(0.0, 30.0))
}

fn danger(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE))
        .fill(ERR)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .min_size(Vec2::new(0.0, 30.0))
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
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(14.0, 6.0);
    });
}

fn main() -> eframe::Result {
    let icon = egui::IconData { rgba: ICON_RGBA.to_vec(), width: 128, height: 128 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Ambyst-App")
            .with_inner_size([760.0, 560.0])
            .with_min_inner_size([560.0, 380.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "Ambyst-App",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
