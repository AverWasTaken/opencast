use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};
use opencast::{
    calculator::{self, Answer},
    index::{Entry, Snapshot},
};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock, mpsc},
    time::{Duration, Instant},
};

const BG: Color32 = Color32::from_rgb(31, 29, 39);
const SURFACE: Color32 = Color32::from_rgb(43, 39, 54);
const SELECTED: Color32 = Color32::from_rgb(65, 57, 80);
const TEXT: Color32 = Color32::from_rgb(239, 236, 246);
const MUTED: Color32 = Color32::from_rgb(165, 157, 180);
const ACCENT: Color32 = Color32::from_rgb(197, 177, 246);
const MINT: Color32 = Color32::from_rgb(151, 218, 190);

#[derive(Default, Serialize, Deserialize)]
struct Config {
    roots: Vec<PathBuf>,
}
enum Event {
    Indexed {
        count: usize,
        skipped: usize,
        elapsed: Duration,
        error: Option<String>,
    },
    Results {
        query: String,
        entries: Vec<Entry>,
        elapsed: Duration,
    },
}
#[derive(PartialEq, Clone, Copy)]
enum Mode {
    All,
    Files,
    Calculator,
}
pub struct OpenCast {
    query: String,
    mode: Mode,
    selected: usize,
    results: Vec<Entry>,
    answer: Option<Answer>,
    calc_error: Option<String>,
    events: mpsc::Receiver<Event>,
    search: mpsc::Sender<String>,
    scan: mpsc::Sender<Vec<PathBuf>>,
    roots: Vec<PathBuf>,
    config_path: PathBuf,
    root_text: String,
    settings: bool,
    actions: bool,
    count: usize,
    indexing: bool,
    index_status: String,
    search_ms: f64,
    notice: Option<(String, Instant)>,
    focus_search: bool,
    pending: bool,
    scroll_selected: bool,
}
impl OpenCast {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = SURFACE;
        style.visuals.extreme_bg_color = BG;
        style.visuals.selection.bg_fill = SELECTED;
        style.visuals.widgets.inactive.bg_fill = SURFACE;
        style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
        style.visuals.widgets.hovered.bg_fill = SELECTED;
        style.visuals.widgets.active.bg_fill = SELECTED;
        style.visuals.window_corner_radius = 14.into();
        style.spacing.item_spacing = Vec2::new(10.0, 10.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        ctx.set_style(style);
        let data = directories::ProjectDirs::from("org", "OpenCast", "OpenCast")
            .map(|p| p.data_local_dir().to_owned())
            .unwrap_or_else(|| std::env::temp_dir().join("opencast"));
        let config_path = data.join("config.json");
        let default_roots = directories::UserDirs::new()
            .map(|u| {
                [u.document_dir(), u.download_dir(), u.desktop_dir()]
                    .into_iter()
                    .flatten()
                    .filter(|p| p.is_dir())
                    .map(|p| p.to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let config = std::fs::read(&config_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Config>(&b).ok())
            .unwrap_or(Config {
                roots: default_roots,
            });
        let roots = config.roots;
        let shared = Arc::new(RwLock::new(Arc::new(Snapshot::default())));
        let (send, events) = mpsc::channel();
        let (search, requests) = mpsc::channel::<String>();
        let (scan, scans) = mpsc::channel::<Vec<PathBuf>>();
        let index = shared.clone();
        let output = send.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            while let Ok(mut query) = requests.recv() {
                while let Ok(newer) = requests.try_recv() {
                    query = newer;
                }
                let snapshot = index.read().unwrap().clone();
                let start = Instant::now();
                let entries = snapshot.search(&query, 40);
                if output
                    .send(Event::Results {
                        query,
                        entries,
                        elapsed: start.elapsed(),
                    })
                    .is_err()
                {
                    break;
                }
                repaint.request_repaint();
            }
        });
        let index = shared;
        let repaint = ctx.clone();
        let mut scan_roots = roots.clone();
        std::thread::spawn(move || {
            let cache_path = data.join("index.json");
            if let Ok(cached) = Snapshot::load(&cache_path)
                && cached.roots == scan_roots
            {
                let count = cached.entries.len();
                *index.write().unwrap() = Arc::new(cached);
                let _ = send.send(Event::Indexed {
                    count,
                    skipped: 0,
                    elapsed: Duration::ZERO,
                    error: None,
                });
                repaint.request_repaint();
            }
            loop {
                let start = Instant::now();
                let snapshot = Snapshot::scan(&scan_roots);
                let error = snapshot.save(&cache_path).err();
                let count = snapshot.entries.len();
                let skipped = snapshot.skipped;
                *index.write().unwrap() = Arc::new(snapshot);
                if send
                    .send(Event::Indexed {
                        count,
                        skipped,
                        elapsed: start.elapsed(),
                        error,
                    })
                    .is_err()
                {
                    break;
                }
                repaint.request_repaint();
                match scans.recv_timeout(Duration::from_secs(60)) {
                    Ok(new_roots) => {
                        scan_roots = new_roots;
                        while let Ok(newer) = scans.try_recv() {
                            scan_roots = newer;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        let root_text = roots
            .iter()
            .map(|r| r.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        let settings = roots.is_empty();
        Self {
            query: String::new(),
            mode: Mode::All,
            selected: 0,
            results: vec![],
            answer: None,
            calc_error: None,
            events,
            search,
            scan,
            roots,
            config_path,
            root_text,
            settings,
            actions: false,
            count: 0,
            indexing: true,
            index_status: "Building your file index…".into(),
            search_ms: 0.0,
            notice: None,
            focus_search: !settings,
            pending: false,
            scroll_selected: false,
        }
    }
    fn refresh(&mut self) {
        self.selected = 0;
        self.results.clear();
        self.pending = true;
        match calculator::calculate(&self.query) {
            Ok(answer) => {
                self.answer = answer;
                self.calc_error = None;
            }
            Err(error) => {
                self.answer = None;
                self.calc_error = Some(error);
            }
        }
        let _ = self.search.send(self.query.clone());
    }
    fn notify(&mut self, message: impl Into<String>) {
        self.notice = Some((message.into(), Instant::now()));
    }
    fn copy(&mut self, ctx: &egui::Context) {
        if self.mode != Mode::Files
            && let Some(answer) = &self.answer
        {
            ctx.copy_text(answer.display());
            self.notify("Result copied");
            return;
        }
        if let Some(entry) = self.results.get(self.selected) {
            ctx.copy_text(entry.path.to_string_lossy().into());
            self.notify("Path copied");
        }
    }
    fn activate(&mut self, ctx: &egui::Context) {
        if self.mode != Mode::Files && self.answer.is_some() {
            self.copy(ctx);
            return;
        }
        if self.mode == Mode::Calculator {
            return;
        }
        if let Some(entry) = self.results.get(self.selected) {
            match open::that_detached(&entry.path) {
                Ok(_) => self.notify("Opened with your default application"),
                Err(e) => self.notify(format!("Could not open file: {e}")),
            }
        }
    }
    fn save_roots(&mut self) {
        let mut roots = Vec::new();
        for line in self
            .root_text
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let path = PathBuf::from(line);
            if !path.is_absolute() || !path.is_dir() {
                self.notify(format!("Enter an existing absolute folder path: {line}"));
                return;
            }
            if !roots.contains(&path) {
                roots.push(path);
            }
        }
        let result = (|| -> Result<(), String> {
            std::fs::create_dir_all(self.config_path.parent().unwrap())
                .map_err(|e| e.to_string())?;
            std::fs::write(
                &self.config_path,
                serde_json::to_vec_pretty(&Config {
                    roots: roots.clone(),
                })
                .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())
        })();
        match result {
            Ok(()) => {
                self.roots = roots;
                self.results.clear();
                self.settings = false;
                self.focus_search = true;
                self.reindex();
            }
            Err(e) => self.notify(format!("Could not save folders: {e}")),
        }
    }
    fn reindex(&mut self) {
        self.indexing = true;
        self.index_status = "Updating your file index…".into();
        let _ = self.scan.send(self.roots.clone());
    }
    fn examples(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        ui.label(
            RichText::new("A little less clicking.")
                .size(23.0)
                .color(TEXT),
        );
        ui.label(RichText::new("Find a file. Work out a number. Keep moving.").color(MUTED));
        ui.add_space(15.0);
        for (icon, title, example) in [
            ("=", "Calculate", "(24 + 8) / 2"),
            ("<>", "Convert units", "10 km to mi"),
            ("C", "Convert temperature", "72 f to c"),
        ] {
            let response = egui::Frame::new()
                .fill(SURFACE)
                .corner_radius(10)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(icon).size(22.0).color(ACCENT));
                        ui.add_space(8.0);
                        ui.label(title);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(example).monospace().color(MUTED));
                        });
                    });
                })
                .response;
            if ui
                .interact(response.rect, response.id.with(title), egui::Sense::click())
                .clicked()
            {
                self.query = example.into();
                self.refresh();
                self.focus_search = true;
            }
            ui.add_space(2.0);
        }
    }
    fn file_rows(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if self.query.is_empty() {
                    "RECENTLY MODIFIED"
                } else {
                    "FILES & FOLDERS"
                })
                .size(11.0)
                .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} results · {:.1} ms",
                        self.results.len(),
                        self.search_ms
                    ))
                    .size(11.0)
                    .color(MUTED),
                );
            });
        });
        ui.add_space(5.0);
        if self.results.is_empty() {
            ui.add_space(25.0);
            ui.label(
                RichText::new(if self.pending {
                    "Searching…"
                } else if self.indexing {
                    "Your files are on their way…"
                } else {
                    "No files found"
                })
                .size(21.0),
            );
            ui.label(
                RichText::new("Try part of a file name, or add a folder in Settings.").color(MUTED),
            );
        }
        let mut activate = false;
        for (i, entry) in self.results.iter().enumerate() {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 62.0), egui::Sense::click());
            if self.selected == i || response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    10,
                    if self.selected == i {
                        SELECTED
                    } else {
                        SURFACE
                    },
                );
            }
            let icon_rect =
                egui::Rect::from_min_size(rect.min + Vec2::new(12.0, 12.0), Vec2::splat(36.0));
            let extension = entry
                .path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_uppercase();
            let (icon, color) = if entry.directory {
                ("DIR", MINT)
            } else {
                (
                    extension.get(..extension.len().min(4)).unwrap_or("FILE"),
                    ACCENT,
                )
            };
            ui.painter()
                .rect_filled(icon_rect, 8, color.gamma_multiply(0.16));
            ui.painter().text(
                icon_rect.center(),
                egui::Align2::CENTER_CENTER,
                icon,
                FontId::monospace(10.0),
                color,
            );
            let mut name_job = egui::text::LayoutJob::simple_singleline(
                entry.name.clone(),
                FontId::proportional(16.0),
                TEXT,
            );
            name_job.wrap.max_width = rect.width() - 85.0;
            name_job.wrap.max_rows = 1;
            name_job.wrap.break_anywhere = true;
            ui.painter().galley(
                rect.min + Vec2::new(62.0, 10.0),
                ui.fonts_mut(|f| f.layout_job(name_job)),
                TEXT,
            );
            let path = entry
                .path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut job =
                egui::text::LayoutJob::simple_singleline(path, FontId::proportional(12.0), MUTED);
            job.wrap.max_width = rect.width() - 85.0;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            ui.painter().galley(
                rect.min + Vec2::new(62.0, 35.0),
                ui.fonts_mut(|f| f.layout_job(job)),
                MUTED,
            );
            if response.clicked() {
                self.selected = i;
            }
            if response.double_clicked() {
                self.selected = i;
                activate = true;
            }
            if self.selected == i && self.scroll_selected {
                response.scroll_to_me(Some(egui::Align::Center));
            }
        }
        self.scroll_selected = false;
        if activate {
            self.activate(ui.ctx());
        }
    }
}
impl eframe::App for OpenCast {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Indexed {
                    count,
                    skipped,
                    elapsed,
                    error,
                } => {
                    self.count = count;
                    self.indexing = false;
                    self.index_status = format!(
                        "{count} items · updated in {:.1}s · {skipped} unreadable entries",
                        elapsed.as_secs_f64()
                    );
                    if let Some(error) = error {
                        self.notify(format!("Index works, but could not save cache: {error}"));
                    }
                    self.pending = true;
                    let _ = self.search.send(self.query.clone());
                }
                Event::Results {
                    query,
                    entries,
                    elapsed,
                } if query == self.query => {
                    self.results = entries;
                    self.search_ms = elapsed.as_secs_f64() * 1000.0;
                    self.pending = false;
                    self.selected = self.selected.min(self.results.len().saturating_sub(1));
                }
                _ => (),
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
            self.settings = !self.settings;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K)) {
            self.actions = !self.actions;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            if self.settings || self.actions {
                self.settings = false;
                self.actions = false;
                self.focus_search = true;
            } else if !self.query.is_empty() {
                self.query.clear();
                self.refresh();
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if !self.settings && !self.actions {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                self.scroll_selected = true;
                self.selected = (self.selected + 1).min(self.results.len().saturating_sub(1));
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                self.scroll_selected = true;
                self.selected = self.selected.saturating_sub(1);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                self.activate(ctx);
            }
            if ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
                    egui::Key::C,
                )
            }) {
                self.copy(ctx);
            }
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .corner_radius(18)
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(82, 73, 97)))
                    .inner_margin(20),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                    let c = rect.center();
                    ui.painter().add(egui::Shape::closed_line(
                        vec![
                            c + Vec2::new(0.0, -9.0),
                            c + Vec2::new(9.0, 0.0),
                            c + Vec2::new(0.0, 9.0),
                            c + Vec2::new(-9.0, 0.0),
                        ],
                        Stroke::new(2.0_f32, ACCENT),
                    ));
                    ui.label(RichText::new("OpenCast").size(15.0).strong());
                    ui.label(
                        RichText::new("/  your everyday shortcut")
                            .size(12.0)
                            .color(MUTED),
                    );
                    let drag = ui.allocate_response(
                        Vec2::new((ui.available_width() - 85.0).max(0.0), 22.0),
                        egui::Sense::drag(),
                    );
                    if drag.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    if ui.button("⚙").on_hover_text("Settings · Ctrl+,").clicked() {
                        self.settings = true;
                    }
                    if ui.button("×").on_hover_text("Close OpenCast").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.add_space(19.0);
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(32.0, 34.0), egui::Sense::hover());
                    ui.painter().circle_stroke(
                        rect.min + Vec2::new(13.0, 13.0),
                        8.0,
                        Stroke::new(2.0_f32, ACCENT),
                    );
                    ui.painter().line_segment(
                        [
                            rect.min + Vec2::new(19.0, 19.0),
                            rect.min + Vec2::new(26.0, 26.0),
                        ],
                        Stroke::new(2.0_f32, ACCENT),
                    );
                    let input = ui.add_sized(
                        [ui.available_width() - 40.0, 42.0],
                        egui::TextEdit::singleline(&mut self.query)
                            .id_salt("search")
                            .font(FontId::proportional(25.0))
                            .frame(false)
                            .hint_text("Search files or calculate…"),
                    );
                    if self.focus_search {
                        input.request_focus();
                        self.focus_search = false;
                    }
                    if input.changed() {
                        self.refresh();
                    }
                    if !self.query.is_empty() && ui.small_button("×").clicked() {
                        self.query.clear();
                        self.refresh();
                        self.focus_search = true;
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    for (mode, title) in [
                        (Mode::All, "All"),
                        (Mode::Files, "Files"),
                        (Mode::Calculator, "Calculator"),
                    ] {
                        if ui.selectable_label(self.mode == mode, title).clicked() {
                            self.mode = mode;
                            self.selected = 0;
                            self.focus_search = true;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("LOCAL  /  OFFLINE").size(10.0).color(MUTED));
                    });
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);
                let height = (ui.available_height() - 49.0).max(100.0);
                egui::ScrollArea::vertical()
                    .id_salt("results")
                    .auto_shrink([false, false])
                    .max_height(height)
                    .min_scrolled_height(height)
                    .show(ui, |ui| {
                        if self.mode != Mode::Files && self.answer.is_some() {
                            let answer = self.answer.as_ref().unwrap().display();
                            ui.add_space(14.0);
                            egui::Frame::new()
                                .fill(SURFACE)
                                .corner_radius(14)
                                .inner_margin(24)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new("CALCULATOR").size(11.0).color(ACCENT));
                                    ui.add_space(14.0);
                                    ui.label(RichText::new(&self.query).size(17.0).color(MUTED));
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(answer).monospace().size(40.0).color(TEXT),
                                    );
                                    ui.add_space(18.0);
                                    if ui.button("Copy result    Enter").clicked() {
                                        self.copy(ctx);
                                    }
                                });
                            ui.add_space(16.0);
                            ui.label(
                                RichText::new("Calculated on your device. No connection needed.")
                                    .size(12.0)
                                    .color(MUTED),
                            );
                        } else if self.mode == Mode::Calculator {
                            if let Some(error) = &self.calc_error {
                                ui.colored_label(ACCENT, error);
                            }
                            self.examples(ui);
                        } else if self.query.is_empty() && self.results.is_empty() {
                            self.examples(ui);
                        } else {
                            if let Some(error) = &self.calc_error {
                                ui.colored_label(ACCENT, error);
                            }
                            self.file_rows(ui);
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        dot.center(),
                        3.0,
                        if self.indexing { ACCENT } else { MINT },
                    );
                    ui.label(
                        RichText::new(if self.indexing {
                            "Indexing…".into()
                        } else {
                            format!("{} indexed", self.count)
                        })
                        .size(11.0)
                        .color(MUTED),
                    )
                    .on_hover_text(&self.index_status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Actions   Ctrl K").clicked() {
                            self.actions = !self.actions;
                        }
                        if ui
                            .button(if self.answer.is_some() && self.mode != Mode::Files {
                                "Copy result  Enter"
                            } else {
                                "Open  Enter"
                            })
                            .clicked()
                        {
                            self.activate(ctx);
                        }
                        ui.label(RichText::new("Up / Down select").size(11.0).color(MUTED));
                    });
                });
            });
        if self.settings {
            egui::Window::new("Search settings").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).default_width(510.0).show(ctx, |ui| {
                ui.label("Folders to index");
                #[cfg(target_os = "windows")]
                if ui.button("Add folder…").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        if !self.root_text.is_empty() { self.root_text.push('\n'); }
                        self.root_text.push_str(&folder.to_string_lossy());
                }
                ui.label(RichText::new("One full folder path per line. Subfolders are included.").size(13.0).color(MUTED));
                ui.add_sized([490.0, 145.0], egui::TextEdit::multiline(&mut self.root_text).font(egui::TextStyle::Monospace).hint_text("C:\\Users\\you\\Documents"));
                ui.label(RichText::new("Only names and paths are indexed, never file contents.\nHidden files, AppData, node_modules and target are skipped.\nThe index refreshes on launch and every 60 seconds.").size(12.0).color(MUTED));
                ui.horizontal(|ui| {
                    if ui.button("Save folders").clicked() { self.save_roots(); }
                    if ui.button("Rebuild index").clicked() { self.reindex(); }
                    if ui.button("Done").clicked() { self.settings = false; self.focus_search = true; }
                });
            });
        }
        if self.actions {
            egui::Window::new("Actions")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-28.0, -64.0])
                .default_width(270.0)
                .show(ctx, |ui| {
                    if ui
                        .button("Open / copy result                    Enter")
                        .clicked()
                    {
                        self.activate(ctx);
                        self.actions = false;
                    }
                    if ui
                        .button("Copy result or path            Ctrl Shift C")
                        .clicked()
                    {
                        self.copy(ctx);
                        self.actions = false;
                    }
                    if ui.button("Open containing folder").clicked() {
                        if let Some(path) = self
                            .results
                            .get(self.selected)
                            .and_then(|e| e.path.parent())
                            && let Err(e) = open::that_detached(path)
                        {
                            self.notify(format!("Could not open folder: {e}"));
                        }
                        self.actions = false;
                    }
                    ui.separator();
                    if ui.button("Rebuild file index").clicked() {
                        self.reindex();
                        self.actions = false;
                    }
                    if ui
                        .button("Search settings                         Ctrl ,")
                        .clicked()
                    {
                        self.settings = true;
                        self.actions = false;
                    }
                });
        }
        if let Some((message, since)) = &self.notice {
            if since.elapsed() < Duration::from_secs(5) {
                egui::Area::new("notice".into())
                    .order(egui::Order::Foreground)
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -64.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style())
                            .inner_margin(12)
                            .show(ui, |ui| {
                                ui.label(message);
                            });
                    });
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
                self.notice = None;
            }
        }
    }
}
