//! wex-gui: one plain window. Built only with `--features gui`; the command line tool never links it.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Frame, Layout, RichText, ScrollArea, Sense, Stroke,
    TextEdit, vec2,
};
use wex::apps::{self, App};
use wex::run;

const TILE: f32 = 96.0;
const LOG_LIMIT: u64 = 256 * 1024;

/// What a background thread tells the window.
enum Event {
    Line(String),
    Changed,
}

#[derive(PartialEq)]
enum Tab {
    Output,
    Crashes,
}

/// A small in-window file browser, so `Add` looks the same on every desktop.
struct Picker {
    dir: PathBuf,
    entries: Vec<(String, bool)>, // (name, is_dir): folders first, then .exe files
}

impl Picker {
    fn open(dir: PathBuf) -> Self {
        let mut entries: Vec<_> = fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_dir = entry.path().is_dir();
                let is_exe = Path::new(&name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
                (!name.starts_with('.') && (is_dir || is_exe)).then_some((name, is_dir))
            })
            .collect();
        entries.sort_by_cached_key(|(name, is_dir)| (!is_dir, name.to_lowercase()));
        Self { dir, entries }
    }
}

struct Gui {
    apps: Vec<App>,
    crashes: Vec<(String, PathBuf)>,
    shown: Option<(PathBuf, String)>, // the selected crash log and its text
    output: String,
    tab: Tab,
    picker: Option<Picker>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Gui {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = Color32::from_gray(70);
        visuals.hyperlink_color = Color32::from_gray(200);
        cc.egui_ctx.set_visuals(visuals);

        let (tx, rx) = channel();
        let mut gui = Self {
            apps: Vec::new(),
            crashes: Vec::new(),
            shown: None,
            output: String::new(),
            tab: Tab::Output,
            picker: None,
            tx,
            rx,
        };
        gui.refresh();
        gui
    }

    /// Rescans installed apps and failed runs. Both are small folder reads.
    fn refresh(&mut self) {
        self.apps = wex::prefixes_dir().map_or_else(Vec::new, |dir| apps::discover(&dir));

        let mut crashes: Vec<_> = wex::logs_dir()
            .and_then(|dir| fs::read_dir(dir).ok())
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "log") && wex::log_failed(path)
            })
            .filter_map(|path| {
                let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
                Some((modified, path))
            })
            .collect();
        crashes.sort_by_key(|&(modified, _)| std::cmp::Reverse(modified));
        self.crashes = crashes
            .into_iter()
            .map(|(_, path)| {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                let name = self
                    .apps
                    .iter()
                    .find(|app| app.prefix_name() == stem)
                    .map_or_else(|| stem.to_string(), |app| app.name.clone());
                (name, path)
            })
            .collect();
    }

    fn say(&mut self, text: &str) {
        self.output.push_str(text);
        self.output.push('\n');
    }

    /// Runs `work` on its own thread (Wine blocks until the app closes) and
    /// wakes the window when it sends something.
    fn spawn(&self, ctx: &egui::Context, work: impl FnOnce(&dyn Fn(Event)) + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        thread::spawn(move || {
            work(&|event| {
                let _ = tx.send(event);
                ctx.request_repaint();
            });
        });
    }

    fn launch(&mut self, ctx: &egui::Context, app: App) {
        self.say(&format!("Starting {}...", app.name));
        self.spawn(ctx, move |send| {
            match run::launch(&app) {
                Ok(()) => send(Event::Line(format!("{} closed.", app.name))),
                Err(message) => send(Event::Line(message)),
            }
            send(Event::Changed);
        });
    }

    fn add(&mut self, ctx: &egui::Context, exe: PathBuf) {
        self.say(&format!("Running {}...", exe.display()));
        self.spawn(ctx, move |send| {
            let note = &mut |line: &str| send(Event::Line(line.to_owned()));
            match run::run_program(&exe, note) {
                Ok(installed) => {
                    for app in installed {
                        send(Event::Line(format!("Installed: {}", app.name)));
                    }
                    send(Event::Line("Finished.".into()));
                }
                Err(message) => send(Event::Line(message)),
            }
            send(Event::Changed);
        });
    }

    fn show_crash(&mut self, path: PathBuf) {
        let mut bytes = Vec::new();
        let read = File::open(&path).and_then(|file| file.take(LOG_LIMIT).read_to_end(&mut bytes));
        let mut text = match read {
            Ok(_) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(reason) => format!("Could not read this log: {}.", run::why(&reason)),
        };
        if bytes.len() as u64 >= LOG_LIMIT {
            text.push_str("\n... (log cut short; the full file is on disk)");
        }
        self.shown = Some((path, text));
    }

    fn tiles(&mut self, ui: &mut egui::Ui) {
        if self.apps.is_empty() {
            ui.label("No apps yet. Click Add and choose an installer (.exe).");
            return;
        }
        let mut chosen = None;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for app in &self.apps {
                    let (rect, response) = ui.allocate_exact_size(vec2(TILE, TILE), Sense::click());
                    let edge = if response.hovered() { 200 } else { 90 };
                    let painter = ui.painter();
                    painter.rect_filled(rect, 6.0, Color32::from_gray(38));
                    painter.rect_stroke(
                        rect,
                        6.0,
                        Stroke::new(1.0, Color32::from_gray(edge)),
                        egui::StrokeKind::Inside,
                    );
                    let letter = app.name.chars().next().unwrap_or('?').to_uppercase();
                    painter.text(
                        rect.center_top() + vec2(0.0, 30.0),
                        Align2::CENTER_CENTER,
                        letter.to_string(),
                        FontId::proportional(32.0),
                        Color32::from_gray(220),
                    );
                    let mut job = egui::text::LayoutJob::simple(
                        app.name.clone(),
                        FontId::proportional(12.0),
                        Color32::from_gray(180),
                        TILE - 8.0,
                    );
                    job.wrap.max_rows = 2;
                    job.halign = egui::Align::Center;
                    painter.galley(
                        rect.center_bottom() + vec2(0.0, -34.0),
                        painter.layout_job(job),
                        Color32::from_gray(180),
                    );
                    if response
                        .on_hover_text(format!("{}\n{}", app.name, app.prefix_name()))
                        .clicked()
                    {
                        chosen = Some(app.clone());
                    }
                }
            });
        });
        if let Some(app) = chosen {
            let ctx = ui.ctx().clone();
            self.launch(&ctx, app);
        }
    }

    fn picker_window(&mut self, ctx: &egui::Context) {
        let Some(picker) = &self.picker else { return };
        let (mut go, mut pick, mut open) = (None, None, true);
        egui::Window::new("Add a program")
            .open(&mut open)
            .default_size([420.0, 360.0])
            .show(ctx, |ui| {
                ui.label(picker.dir.display().to_string());
                if ui.button("Up").clicked() {
                    go = picker.dir.parent().map(Path::to_path_buf);
                }
                ui.separator();
                ScrollArea::vertical().show(ui, |ui| {
                    for (name, is_dir) in &picker.entries {
                        let text = if *is_dir {
                            format!("{name}/")
                        } else {
                            name.clone()
                        };
                        if ui.selectable_label(false, text).clicked() {
                            if *is_dir {
                                go = Some(picker.dir.join(name));
                            } else {
                                pick = Some(picker.dir.join(name));
                            }
                        }
                    }
                });
            });
        if let Some(exe) = pick {
            self.picker = None;
            self.add(ctx, exe);
        } else if let Some(dir) = go {
            self.picker = Some(Picker::open(dir));
        } else if !open {
            self.picker = None;
        }
    }

    fn bottom(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Output, "Output");
            let crashes = format!("Crash logs ({})", self.crashes.len());
            ui.selectable_value(&mut self.tab, Tab::Crashes, crashes);
        });
        ui.separator();
        match self.tab {
            Tab::Output => {
                ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        ui.add_sized(
                            ui.available_size(),
                            TextEdit::multiline(&mut self.output.as_str())
                                .desired_width(f32::INFINITY),
                        );
                    });
            }
            Tab::Crashes => self.crash_tab(ui),
        }
    }

    fn crash_tab(&mut self, ui: &mut egui::Ui) {
        if self.crashes.is_empty() {
            ui.label("No crashes recorded.");
            return;
        }
        let mut pick = None;
        let (width, height) = (ui.available_width(), ui.available_height());
        let column = Layout::top_down(Align::Min);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(190.0, height), column, |ui| {
                ScrollArea::vertical().id_salt("crash_list").show(ui, |ui| {
                    for (name, path) in &self.crashes {
                        let selected = self.shown.as_ref().is_some_and(|(shown, _)| shown == path);
                        if ui.selectable_label(selected, name).clicked() {
                            pick = Some(path.clone());
                        }
                    }
                });
            });
            ui.separator();
            ui.allocate_ui_with_layout(vec2(width - 205.0, height), column, |ui| {
                ScrollArea::vertical()
                    .id_salt("crash_text")
                    .show(ui, |ui| match &self.shown {
                        Some((path, text)) => {
                            ui.label(RichText::new(path.display().to_string()).weak());
                            ui.add(
                                TextEdit::multiline(&mut text.as_str())
                                    .desired_width(f32::INFINITY),
                            );
                        }
                        None => {
                            ui.label("Pick a log on the left.");
                        }
                    });
            });
        });
        if let Some(path) = pick {
            self.show_crash(path);
        }
    }
}

impl eframe::App for Gui {
    /// Takes in what the background threads reported, before each draw.
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Line(text) => self.say(&text),
                Event::Changed => changed = true,
            }
        }
        if changed {
            self.refresh();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Add").clicked() {
                    let start = std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_default();
                    let downloads = start.join("Downloads");
                    self.picker = Some(Picker::open(if downloads.is_dir() {
                        downloads
                    } else {
                        start
                    }));
                }
                if ui.button("Refresh").clicked() {
                    self.refresh();
                }
            });
        });
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(190.0)
            .show(ui, |ui| self.bottom(ui));
        egui::CentralPanel::default()
            .frame(Frame::group(ui.style()).fill(Color32::from_gray(24)))
            .show(ui, |ui| self.tiles(ui));
        let ctx = ui.ctx().clone();
        self.picker_window(&ctx);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("wex")
            .with_inner_size([720.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native("wex", options, Box::new(|cc| Ok(Box::new(Gui::new(cc)))))
}
