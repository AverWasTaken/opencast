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

const BG: Color32 = Color32::from_rgb(35, 34, 38);
const SURFACE: Color32 = Color32::from_rgb(43, 41, 43);
const SELECTED: Color32 = Color32::from_rgba_premultiplied(27, 27, 27, 27);
const TEXT: Color32 = Color32::from_rgb(239, 236, 246);
const MUTED: Color32 = Color32::from_rgb(174, 169, 180);
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
    action_query: String,
    action_selected: usize,
    focus_actions: bool,
    backdrop: bool,
}
impl OpenCast {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        #[cfg(windows)]
        let backdrop = window_vibrancy::apply_acrylic(cc, Some((25, 24, 27, 150))).is_ok();
        #[cfg(not(windows))]
        let backdrop = false;
        // Use the platform face without redistributing proprietary system fonts.
        let mut fonts = egui::FontDefinitions::default();
        let system_font = if cfg!(windows) {
            std::env::var_os("WINDIR")
                .map(PathBuf::from)
                .map(|p| p.join("Fonts/segoeui.ttf"))
        } else {
            Some(PathBuf::from(
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ))
        };
        if let Some(bytes) = system_font.and_then(|p| std::fs::read(p).ok()) {
            fonts
                .font_data
                .insert("system".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "system".into());
        }
        ctx.set_fonts(fonts);
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
        style.spacing.item_spacing = Vec2::new(8.0, 1.0);
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
            action_query: String::new(),
            action_selected: 0,
            focus_actions: false,
            backdrop,
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
    fn toggle_actions(&mut self) {
        self.actions = !self.actions;
        self.action_query.clear();
        self.action_selected = 0;
        self.focus_actions = self.actions;
        self.focus_search = !self.actions;
    }
    fn file_rows(&mut self, ui: &mut egui::Ui) {
        if self.results.is_empty() {
            ui.add_space(28.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(if self.pending || self.indexing {
                        "Indexing your files…"
                    } else {
                        "No files found"
                    })
                    .size(17.0),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Try another name, or add a folder in Search settings.")
                        .color(MUTED),
                );
                if ui
                    .add(egui::Button::new("Search settings").frame(false))
                    .clicked()
                {
                    self.settings = true;
                }
            });
        }
        let mut activate = false;
        for (i, entry) in self.results.iter().enumerate() {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 43.0), egui::Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    self.selected == i,
                    &entry.name,
                )
            });
            if self.selected == i || response.hovered() {
                ui.painter().rect_filled(
                    rect,
                    12,
                    if self.selected == i {
                        SELECTED
                    } else {
                        Color32::from_white_alpha(10)
                    },
                );
            }
            let (kind, color, glyph) = file_kind(entry);
            file_icon(
                ui.painter(),
                egui::Rect::from_center_size(rect.min + Vec2::new(22.0, 21.5), Vec2::splat(25.0)),
                color,
                glyph,
            );
            let label = ui
                .painter()
                .layout_no_wrap(kind.into(), FontId::proportional(15.0), MUTED);
            let type_width = label.size().x;
            ui.painter().galley(
                egui::pos2(
                    rect.right() - type_width - 12.0,
                    rect.center().y - label.size().y / 2.0,
                ),
                label,
                MUTED,
            );
            clipped_text(
                ui,
                &entry.name,
                rect.min + Vec2::new(48.0, 11.5),
                rect.width() - type_width - 82.0,
                15.5,
                TEXT,
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
            if !self.actions && !self.settings {
                response.on_hover_text(entry.path.to_string_lossy());
            }
        }
        self.scroll_selected = false;
        if activate {
            self.activate(ui.ctx());
        }
    }
    fn examples(&mut self, ui: &mut egui::Ui) {
        for (title, expression, kind) in [
            ("Calculator", "(24 + 8) / 2", "Calculation"),
            ("Convert units", "10 km to mi", "Conversion"),
            ("Convert temperature", "72 f to c", "Conversion"),
        ] {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 43.0), egui::Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 12, SELECTED);
            }
            file_icon(
                ui.painter(),
                egui::Rect::from_center_size(rect.min + Vec2::new(22.0, 21.5), Vec2::splat(25.0)),
                Color32::from_rgb(224, 151, 66),
                3,
            );
            ui.painter().text(
                rect.min + Vec2::new(48.0, 21.5),
                egui::Align2::LEFT_CENTER,
                title,
                FontId::proportional(15.5),
                TEXT,
            );
            ui.painter().text(
                rect.right_center() - Vec2::new(12.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                kind,
                FontId::proportional(15.0),
                MUTED,
            );
            if response.clicked() {
                self.query = expression.into();
                self.refresh();
                self.focus_search = true;
            }
        }
    }
    fn action_items(&self) -> Vec<(u8, &'static str, &'static str)> {
        let mut items = vec![];
        if self.answer.is_some() && self.mode != Mode::Files {
            items.push((0, "Copy result", "Enter"));
        } else if self.mode != Mode::Calculator && !self.results.is_empty() {
            items.extend([
                (0, "Open file", "Enter"),
                (1, "Copy path", "Ctrl Shift C"),
                (2, "Open containing folder", ""),
            ]);
        }
        items.extend([
            (3, "Search settings", "Ctrl ,"),
            (4, "Rebuild file index", ""),
            (5, "Search everything", ""),
            (6, "Search files only", ""),
            (7, "Calculator", ""),
            (8, "Quit OpenCast", ""),
        ]);
        let query = self.action_query.to_lowercase();
        items.retain(|(_, label, _)| label.to_lowercase().contains(&query));
        items
    }
    fn run_action(&mut self, id: u8, ctx: &egui::Context) {
        match id {
            0 => self.activate(ctx),
            1 => self.copy(ctx),
            2 => {
                if let Some(path) = self
                    .results
                    .get(self.selected)
                    .and_then(|e| e.path.parent())
                    && let Err(e) = open::that_detached(path)
                {
                    self.notify(format!("Could not open folder: {e}"));
                }
            }
            3 => self.settings = true,
            4 => self.reindex(),
            5..=7 => {
                self.mode = match id {
                    6 => Mode::Files,
                    7 => Mode::Calculator,
                    _ => Mode::All,
                };
                self.selected = 0;
            }
            8 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            _ => (),
        }
        self.actions = false;
        self.focus_search = !self.settings;
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
                        self.notify(format!("Could not save index cache: {error}"));
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
            self.actions = false;
        }
        if !self.settings
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::K))
        {
            self.toggle_actions();
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
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let outer = ui.max_rect().shrink(1.0);
                glass(ui.painter(), outer, self.backdrop);
                ui.painter().rect_stroke(
                    outer,
                    17,
                    Stroke::new(1.0_f32, Color32::from_white_alpha(48)),
                    egui::StrokeKind::Inside,
                );
                let header = egui::Rect::from_min_max(
                    outer.min,
                    egui::pos2(outer.right(), outer.top() + 60.0),
                );
                let footer = egui::Rect::from_min_max(
                    egui::pos2(outer.left(), outer.bottom() - 44.0),
                    outer.max,
                );
                ui.painter().line_segment(
                    [header.left_bottom(), header.right_bottom()],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(18)),
                );
                ui.painter().rect_filled(
                    footer,
                    egui::CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 16,
                        se: 16,
                    },
                    Color32::from_rgba_unmultiplied(26, 26, 27, 215),
                );
                ui.painter().line_segment(
                    [footer.left_top(), footer.right_top()],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(22)),
                );
                let drag = ui.interact(
                    egui::Rect::from_min_max(
                        header.min,
                        header.min + Vec2::new(header.width(), 12.0),
                    ),
                    ui.id().with("drag"),
                    egui::Sense::drag(),
                );
                if drag.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(header.shrink2(Vec2::new(18.0, 17.0))),
                    |ui| {
                        let hint = match self.mode {
                            Mode::Files => "Search files…",
                            Mode::Calculator => "Calculate or convert…",
                            Mode::All => "Search files or calculate…",
                        };
                        let input = ui.add_enabled(
                            !self.actions && !self.settings,
                            egui::TextEdit::singleline(&mut self.query)
                                .id_salt("search")
                                .interactive(!self.actions && !self.settings)
                                .font(FontId::proportional(20.0))
                                .frame(false)
                                .margin(Vec2::ZERO)
                                .desired_width(ui.available_width())
                                .hint_text(hint),
                        );
                        if self.focus_search && !self.settings {
                            input.request_focus();
                            self.focus_search = false;
                        }
                        if input.changed() {
                            self.refresh();
                        }
                    },
                );
                let content = egui::Rect::from_min_max(
                    header.left_bottom() + Vec2::new(8.0, 9.0),
                    footer.right_top() - Vec2::new(8.0, 8.0),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
                    ui.set_clip_rect(content);
                    egui::ScrollArea::vertical()
                        .id_salt("files")
                        .auto_shrink([false, false])
                        .max_height(content.height())
                        .show(ui, |ui| {
                            if self.mode != Mode::Files
                                && let Some(answer) = &self.answer
                            {
                                let answer = answer.display();
                                ui.add_space(24.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(18.0);
                                    ui.label(RichText::new("Calculator").size(14.0).color(MUTED));
                                });
                                ui.add_space(20.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(18.0);
                                    ui.label(RichText::new(answer).size(38.0));
                                });
                            } else {
                                if let Some(error) = &self.calc_error {
                                    ui.add_space(10.0);
                                    ui.label(RichText::new(error).color(MUTED));
                                    ui.add_space(10.0);
                                }
                                if self.mode == Mode::Calculator
                                    || (self.query.is_empty() && self.results.is_empty())
                                {
                                    self.examples(ui);
                                } else {
                                    self.file_rows(ui);
                                }
                            }
                        });
                });
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(footer.shrink2(Vec2::new(16.0, 9.0))),
                    |ui| {
                        ui.horizontal_centered(|ui| {
                            let (dot, response) =
                                ui.allocate_exact_size(Vec2::new(10.0, 20.0), egui::Sense::hover());
                            ui.painter().circle_filled(
                                dot.center(),
                                2.5,
                                if self.indexing {
                                    MUTED
                                } else {
                                    MINT.gamma_multiply(0.7)
                                },
                            );
                            response.on_hover_text(format!(
                                "{}\nLast search: {:.1} ms",
                                self.index_status, self.search_ms
                            ));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if footer_action(ui, "Actions", &["Ctrl", "K"]).clicked() {
                                        self.toggle_actions();
                                    }
                                    ui.label(RichText::new("│").color(Color32::from_gray(80)));
                                    let label = if self.answer.is_some() && self.mode != Mode::Files
                                    {
                                        "Copy result"
                                    } else {
                                        "Open file"
                                    };
                                    if footer_action(ui, label, &["Enter"]).clicked()
                                        && !self.settings
                                        && !self.actions
                                    {
                                        self.activate(ctx);
                                    }
                                },
                            );
                        });
                    },
                );
            });
        if self.actions {
            let items = self.action_items();
            self.action_selected = self.action_selected.min(items.len().saturating_sub(1));
            let mut navigated = false;
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                self.action_selected =
                    (self.action_selected + 1).min(items.len().saturating_sub(1));
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)) {
                navigated = true;
                self.action_selected = self.action_selected.saturating_sub(1);
            }
            let mut chosen =
                if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    items.get(self.action_selected).map(|item| item.0)
                } else {
                    None
                };
            let title = if self.answer.is_some() && self.mode != Mode::Files {
                "Calculator"
            } else {
                self.results
                    .get(self.selected)
                    .map_or("OpenCast", |e| e.name.as_str())
            }
            .to_owned();
            egui::Area::new("actions".into())
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-15.0, -53.0])
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(39, 35, 34))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(86, 80, 79)))
                        .corner_radius(22)
                        .shadow(egui::Shadow {
                            offset: [0, 8],
                            blur: 28,
                            spread: 2,
                            color: Color32::from_black_alpha(90),
                        })
                        .inner_margin(9)
                        .show(ui, |ui| {
                            ui.set_width(360.0);
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                ui.add_space(10.0);
                                ui.label(RichText::new(title).size(14.0).color(MUTED));
                            });
                            ui.add_space(12.0);
                            egui::ScrollArea::vertical()
                                .id_salt("actions-list")
                                .max_height(245.0)
                                .show(ui, |ui| {
                                    let mut previous_group = None;
                                    for (i, (id, label, keys)) in items.iter().enumerate() {
                                        let group = if *id < 3 {
                                            0
                                        } else if *id < 5 {
                                            1
                                        } else {
                                            2
                                        };
                                        if previous_group.is_some_and(|g| g != group) {
                                            ui.add_space(7.0);
                                            ui.separator();
                                            ui.add_space(7.0);
                                        }
                                        previous_group = Some(group);
                                        let (rect, response) = ui.allocate_exact_size(
                                            Vec2::new(ui.available_width(), 43.0),
                                            egui::Sense::click(),
                                        );
                                        response.widget_info(|| {
                                            egui::WidgetInfo::selected(
                                                egui::WidgetType::SelectableLabel,
                                                true,
                                                i == self.action_selected,
                                                *label,
                                            )
                                        });
                                        if i == self.action_selected || response.hovered() {
                                            ui.painter().rect_filled(rect, 12, SELECTED);
                                        }
                                        action_icon(
                                            ui.painter(),
                                            rect.min + Vec2::new(20.0, 21.5),
                                            *id,
                                        );
                                        ui.painter().text(
                                            rect.min + Vec2::new(44.0, 21.5),
                                            egui::Align2::LEFT_CENTER,
                                            *label,
                                            FontId::proportional(15.0),
                                            TEXT,
                                        );
                                        let mut right = rect.right() - 9.0;
                                        for key in keys.split_whitespace().rev() {
                                            right = keycap(
                                                ui.painter(),
                                                egui::pos2(right, rect.center().y),
                                                key,
                                            );
                                        }
                                        if response.clicked() {
                                            chosen = Some(*id);
                                        }
                                        if i == self.action_selected && navigated {
                                            response.scroll_to_me(None);
                                        }
                                    }
                                    if items.is_empty() {
                                        ui.label(RichText::new("No matching actions").color(MUTED));
                                    }
                                });
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(8.0);
                            let input = ui.add(
                                egui::TextEdit::singleline(&mut self.action_query)
                                    .id_salt("action-search")
                                    .font(FontId::proportional(16.0))
                                    .hint_text("Search for actions…")
                                    .frame(false)
                                    .desired_width(f32::INFINITY)
                                    .margin(Vec2::new(4.0, 3.0)),
                            );
                            if self.focus_actions {
                                input.request_focus();
                                self.focus_actions = false;
                            }
                            if input.changed() {
                                self.action_selected = 0;
                            }
                            ui.add_space(4.0);
                        });
                });
            if let Some(id) = chosen {
                self.run_action(id, ctx);
            }
        }
        if self.settings {
            egui::Window::new("Search settings")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .default_width(490.0)
                .show(ctx, |ui| {
                    ui.add_space(10.0);
                    ui.label("Folders to index");
                    ui.add_space(8.0);
                    #[cfg(windows)]
                    if ui.button("Add folder…").clicked()
                        && let Some(folder) = rfd::FileDialog::new().pick_folder()
                    {
                        if !self.root_text.is_empty() {
                            self.root_text.push('\n');
                        }
                        self.root_text.push_str(&folder.to_string_lossy());
                    }
                    ui.label(
                        RichText::new("One full folder path per line. Subfolders are included.")
                            .size(13.0)
                            .color(MUTED),
                    );
                    ui.add_space(8.0);
                    ui.add_sized(
                        [480.0, 130.0],
                        egui::TextEdit::multiline(&mut self.root_text)
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("File names and paths only. Refreshes every 60 seconds.")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save folders").clicked() {
                            self.save_roots();
                        }
                        if ui.button("Rebuild index").clicked() {
                            self.reindex();
                        }
                        if ui.button("Done").clicked() {
                            self.settings = false;
                            self.focus_search = true;
                        }
                    });
                });
        }
        if let Some((message, since)) = &self.notice {
            if since.elapsed() < Duration::from_secs(5) {
                egui::Area::new("notice".into())
                    .order(egui::Order::Tooltip)
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -54.0])
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

fn clipped_text(ui: &egui::Ui, text: &str, pos: egui::Pos2, width: f32, size: f32, color: Color32) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), FontId::proportional(size), color);
    job.wrap.max_width = width.max(10.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.painter()
        .galley(pos, ui.fonts_mut(|f| f.layout_job(job)), color);
}
fn keycap(p: &egui::Painter, right: egui::Pos2, label: &str) -> f32 {
    let text = p.layout_no_wrap(label.into(), FontId::proportional(12.0), MUTED);
    let size = Vec2::new((text.size().x + 12.0).max(25.0), 24.0);
    let rect = egui::Rect::from_min_size(egui::pos2(right.x - size.x, right.y - 12.0), size);
    p.rect_filled(rect, 7, Color32::from_white_alpha(18));
    p.galley(rect.center() - text.size() / 2.0, text, MUTED);
    rect.left() - 4.0
}
fn footer_action(ui: &mut egui::Ui, label: &str, keys: &[&str]) -> egui::Response {
    let text = ui
        .painter()
        .layout_no_wrap(label.into(), FontId::proportional(14.0), MUTED);
    let key_width: f32 = keys
        .iter()
        .map(|k| {
            (ui.painter()
                .layout_no_wrap((*k).into(), FontId::proportional(12.0), MUTED)
                .size()
                .x
                + 12.0)
                .max(25.0)
                + 4.0
        })
        .sum();
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(text.size().x + key_width + 12.0, 26.0),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    ui.painter().galley(
        rect.left_center() - Vec2::new(0.0, text.size().y / 2.0),
        text,
        if response.hovered() { TEXT } else { MUTED },
    );
    let mut right = rect.right();
    for key in keys.iter().rev() {
        right = keycap(ui.painter(), egui::pos2(right, rect.center().y), key);
    }
    response
}
fn file_kind(entry: &Entry) -> (&'static str, Color32, u8) {
    if entry.directory {
        return ("Folder", Color32::from_rgb(77, 162, 233), 1);
    }
    match entry
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "pdf" => ("PDF document", Color32::from_rgb(216, 86, 83), 0),
        "xlsx" | "csv" | "xls" => ("Spreadsheet", Color32::from_rgb(65, 164, 112), 3),
        "png" | "jpg" | "jpeg" | "svg" | "fig" => ("Image", Color32::from_rgb(158, 119, 219), 2),
        "rs" | "js" | "ts" | "py" | "json" | "toml" => ("Code", Color32::from_rgb(75, 148, 220), 4),
        "md" | "txt" | "docx" => ("Document", Color32::from_rgb(129, 163, 206), 0),
        _ => ("File", Color32::from_rgb(146, 148, 159), 0),
    }
}
fn file_icon(p: &egui::Painter, r: egui::Rect, color: Color32, kind: u8) {
    let s = Stroke::new(1.25_f32, Color32::from_white_alpha(230));
    p.rect_filled(r, 6, color);
    let c = r.center();
    match kind {
        1 => {
            p.rect_stroke(
                r.shrink2(Vec2::new(4.0, 6.0)),
                2,
                s,
                egui::StrokeKind::Inside,
            );
            p.line_segment([c + Vec2::new(-8.0, -6.0), c + Vec2::new(-1.0, -6.0)], s);
        }
        2 => {
            p.circle_filled(c + Vec2::new(4.0, -4.0), 2.0, Color32::WHITE);
            p.add(egui::Shape::line(
                vec![
                    c + Vec2::new(-8.0, 6.0),
                    c + Vec2::new(-3.0, -1.0),
                    c + Vec2::new(2.0, 4.0),
                    c + Vec2::new(5.0, 1.0),
                    c + Vec2::new(8.0, 6.0),
                ],
                s,
            ));
        }
        3 => {
            for offset in [-5.0, 0.0, 5.0] {
                p.line_segment([c + Vec2::new(-7.0, offset), c + Vec2::new(7.0, offset)], s);
            }
            p.line_segment([c + Vec2::new(-2.0, -7.0), c + Vec2::new(-2.0, 7.0)], s);
        }
        4 => {
            p.add(egui::Shape::line(
                vec![
                    c + Vec2::new(-6.0, -5.0),
                    c + Vec2::new(-1.0, 0.0),
                    c + Vec2::new(-6.0, 5.0),
                ],
                s,
            ));
            p.line_segment([c + Vec2::new(1.0, 5.0), c + Vec2::new(7.0, 5.0)], s);
        }
        _ => {
            p.rect_stroke(
                r.shrink2(Vec2::new(6.0, 4.0)),
                1,
                s,
                egui::StrokeKind::Inside,
            );
            for y in [-3.0, 1.0, 5.0] {
                p.line_segment([c + Vec2::new(-3.0, y), c + Vec2::new(3.0, y)], s);
            }
        }
    }
}
fn action_icon(p: &egui::Painter, c: egui::Pos2, id: u8) {
    let s = Stroke::new(1.4_f32, TEXT);
    match id {
        3 | 4 => {
            p.circle_stroke(c, 7.0, s);
            p.circle_stroke(c, 2.5, s);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let v = Vec2::angled(a);
                p.line_segment([c + v * 7.0, c + v * 10.0], s);
            }
        }
        1 => {
            p.rect_stroke(
                egui::Rect::from_center_size(c, Vec2::new(12.0, 15.0)),
                2,
                s,
                egui::StrokeKind::Inside,
            );
        }
        8 => {
            p.line_segment([c - Vec2::splat(6.0), c + Vec2::splat(6.0)], s);
            p.line_segment([c + Vec2::new(6.0, -6.0), c + Vec2::new(-6.0, 6.0)], s);
        }
        _ => {
            p.rect_stroke(
                egui::Rect::from_center_size(c, Vec2::new(17.0, 13.0)),
                3,
                s,
                egui::StrokeKind::Inside,
            );
            p.line_segment([c + Vec2::new(-5.0, 2.0), c + Vec2::new(5.0, 2.0)], s);
        }
    }
}
fn glass(p: &egui::Painter, r: egui::Rect, backdrop: bool) {
    // Quiet tinted fallback on systems without a desktop compositor. On Windows
    // acrylic supplies the actual blurred desktop behind a lightly tinted mesh.
    let mut mesh = egui::Mesh::default();
    let color = |pos: egui::Pos2| {
        let x = (pos.x - r.left()) / r.width();
        let y = (pos.y - r.top()) / r.height();
        let left = [48.0 - 17.0 * y, 42.0 - 5.0 * y, 64.0 - 10.0 * y];
        let right = [57.0 - 12.0 * y, 46.0 - 10.0 * y, 39.0 - 3.0 * y];
        Color32::from_rgba_unmultiplied(
            (left[0] * (1.0 - x) + right[0] * x) as u8,
            (left[1] * (1.0 - x) + right[1] * x) as u8,
            (left[2] * (1.0 - x) + right[2] * x) as u8,
            if backdrop { 38 } else { 255 },
        )
    };
    mesh.colored_vertex(r.center(), color(r.center()));
    let radius = 17.0;
    for (corner, start) in [
        (r.left_top() + Vec2::splat(radius), std::f32::consts::PI),
        (
            r.right_top() + Vec2::new(-radius, radius),
            std::f32::consts::PI * 1.5,
        ),
        (r.right_bottom() - Vec2::splat(radius), 0.0),
        (
            r.left_bottom() + Vec2::new(radius, -radius),
            std::f32::consts::FRAC_PI_2,
        ),
    ] {
        for i in 0..=8 {
            let pos = corner
                + Vec2::angled(start + i as f32 / 8.0 * std::f32::consts::FRAC_PI_2) * radius;
            mesh.colored_vertex(pos, color(pos));
        }
    }
    let n = mesh.vertices.len() as u32 - 1;
    for i in 1..=n {
        mesh.add_triangle(0, i, if i == n { 1 } else { i + 1 });
    }
    p.add(mesh);
}
