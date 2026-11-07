//! wex-gui: one plain window. Built only with `--features gui`; the command line tool never links it.

use std::cmp::Reverse;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::mem;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::SystemTime;

use eframe::egui::{
    self, Align, Align2, Color32, ComboBox, FontId, Frame, Key, Layout, RichText, ScrollArea,
    Sense, Stroke, TextEdit, vec2,
};
use wex::apps::{self, App, Target};
use wex::files::{self, SavedFile};
use wex::run;
use wex::shortcut;

const TILE: f32 = 96.0;
const LOG_LIMIT: u64 = 256 * 1024;
const OUTPUT_LIMIT: usize = 200 * 1024;
const FILES_SHOWN: usize = 200;
const WINE_URL: &str = "https://www.winehq.org/download";

/// What a background thread tells the window.
enum Event {
    Line(String),
    /// An app (or installer) running in this prefix has ended.
    Ended(PathBuf),
}

#[derive(PartialEq)]
enum Tab {
    Output,
    Files,
    Crashes,
}

/// Something the user asked for with a tile; acted on after drawing.
enum Act {
    Launch(App),
    Files(App),
    Folder(App),
    Rename(App),
    MenuAdd(App),
    MenuRemove(App),
    Unpin(App),
}

/// The order of the tiles. `AToZ` is the order `apps::discover` already returns.
#[derive(Clone, Copy, PartialEq)]
enum Sort {
    AToZ,
    ZToA,
    Recent,
}

impl Sort {
    const ALL: [Self; 3] = [Self::AToZ, Self::ZToA, Self::Recent];

    fn label(self) -> &'static str {
        match self {
            Self::AToZ => "A to Z",
            Self::ZToA => "Z to A",
            Self::Recent => "Recently played",
        }
    }
}

/// The "Wine is missing" window: shown at start when Wine is not found, and again
/// (with a short extra line) when "Check again" still does not find it.
#[derive(PartialEq)]
enum Notice {
    Hidden,
    Missing,
    StillMissing,
}

/// The Rename window: the app, the text being typed, and whether the box still
/// needs the keyboard focus (true only for the first frame).
struct Renaming {
    app: App,
    text: String,
    focus: bool,
}

/// An app that is running now. `stopped` is set when the user presses Stop, so the
/// end of that run is reported as a stop and not as a crash.
struct Running {
    name: String,
    prefix: PathBuf,
    stopped: Arc<AtomicBool>,
}

/// The Files tab: one app's saved files, with each row's text built once.
struct FilesView {
    app: App,
    title: String,
    rows: Vec<(String, PathBuf)>, // (label, folder to open)
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

fn size_text(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn age_text(time: SystemTime) -> String {
    let secs = time.elapsed().map_or(0, |age| age.as_secs());
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// Fills `view` with the indexes of the apps whose shown name contains `query`
/// (ignoring case and spaces at the ends), in the order `sort` asks for. `apps`
/// comes sorted A to Z by the shown name, so that order needs no work and Z to A is
/// just reversed.
fn arrange(view: &mut Vec<usize>, apps: &[App], query: &str, sort: Sort) {
    let query = query.trim().to_lowercase();
    view.clear();
    view.extend(
        apps.iter()
            .enumerate()
            .filter(|(_, app)| query.is_empty() || app.name.to_lowercase().contains(&query))
            .map(|(index, _)| index),
    );
    match sort {
        Sort::AToZ => {}
        Sort::ZToA => view.reverse(),
        // Newest first; apps never played go last. The sort is stable, so equal
        // times stay in A to Z order.
        Sort::Recent => view.sort_by_key(|&index| Reverse(apps[index].played)),
    }
}

struct Gui {
    apps: Vec<App>,
    /// Which apps are shown as tiles, and in what order: indexes into `apps`.
    /// Rebuilt only when the apps, the search text or the sort changes, never per frame.
    view: Vec<usize>,
    query: String,
    sort: Sort,
    notice: Notice,
    renaming: Option<Renaming>,
    running: Vec<Running>,
    crashes: Vec<(String, PathBuf)>,
    shown: Option<(PathBuf, String)>, // the selected crash log and its text
    files: Option<FilesView>,
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
            view: Vec::new(),
            query: String::new(),
            sort: Sort::AToZ,
            notice: if wex::wine_installed() {
                Notice::Hidden
            } else {
                Notice::Missing
            },
            renaming: None,
            running: Vec::new(),
            crashes: Vec::new(),
            shown: None,
            files: None,
            output: String::new(),
            tab: Tab::Output,
            picker: None,
            tx,
            rx,
        };
        gui.refresh();
        gui
    }

    /// Works out which tiles to show and in what order (see `arrange`).
    fn rebuild_view(&mut self) {
        arrange(&mut self.view, &self.apps, &self.query, self.sort);
    }

    /// Rescans apps and failed runs. Both are small folder reads.
    fn refresh(&mut self) {
        self.apps = wex::prefixes_dir().map_or_else(Vec::new, |dir| apps::discover(&dir));
        self.rebuild_view();

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

    /// Adds a line to Output, keeping only the newest ~100 KB once it passes 200 KB.
    fn say(&mut self, text: &str) {
        if self.output.len() > OUTPUT_LIMIT {
            // Cut just after a newline, which is always a character boundary.
            let from = self.output.len() - OUTPUT_LIMIT / 2;
            let cut = self.output.as_bytes()[from..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(self.output.len(), |at| from + at + 1);
            self.output.drain(..cut);
        }
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

    /// Starts `job` as the running program of `prefix`. One program per prefix:
    /// Wine shares one server per prefix, and Stop ends the whole prefix.
    fn begin(
        &mut self,
        ctx: &egui::Context,
        name: String,
        prefix: PathBuf,
        job: impl FnOnce(&dyn Fn(Event)) -> Result<(), String> + Send + 'static,
    ) {
        if self.running.iter().any(|r| r.prefix == prefix) {
            self.say(&format!("{name} is already running."));
            return;
        }
        let stopped = Arc::new(AtomicBool::new(false));
        self.running.push(Running {
            name: name.clone(),
            prefix: prefix.clone(),
            stopped: Arc::clone(&stopped),
        });
        self.say(&format!("Starting {name}..."));
        self.spawn(ctx, move |send| {
            let result = job(send);
            let line = if stopped.load(Ordering::Relaxed) {
                // A stop is not a crash: say so, and drop the log so Crash logs
                // only lists real failures. Best effort; a leftover log is harmless.
                if let Some(log) = wex::log_path(&prefix) {
                    let _ = fs::remove_file(log);
                }
                format!("{name} stopped.")
            } else {
                match result {
                    Ok(()) => format!("{name} closed."),
                    Err(message) => message,
                }
            };
            send(Event::Line(line));
            send(Event::Ended(prefix));
        });
    }

    fn launch(&mut self, ctx: &egui::Context, app: App) {
        let (name, prefix) = (app.name.clone(), app.prefix.clone());
        self.begin(ctx, name, prefix, move |_| run::launch(&app));
    }

    fn add(&mut self, ctx: &egui::Context, exe: PathBuf) {
        let name = exe
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let prefix = fs::canonicalize(&exe)
            .ok()
            .and_then(|real| wex::prefix_manager(&real));
        let Some(prefix) = prefix else {
            self.say(&format!(
                "wex could not open {}: the file is missing, or wex has no place to keep its files (check HOME).",
                exe.display()
            ));
            return;
        };
        self.begin(ctx, name, prefix, move |send| {
            let note = &mut |line: &str| send(Event::Line(line.to_owned()));
            run::run_program(&exe, note)
        });
    }

    /// Force-quits the running app at `index` (Wine's own kill, per prefix).
    fn stop(&mut self, ctx: &egui::Context, index: usize) {
        let running = &self.running[index];
        running.stopped.store(true, Ordering::Relaxed);
        let (name, prefix) = (running.name.clone(), running.prefix.clone());
        let stopped = Arc::clone(&running.stopped);
        self.say(&format!("Stopping {name}..."));
        self.spawn(ctx, move |send| {
            if let Err(message) = run::stop(&prefix) {
                stopped.store(false, Ordering::Relaxed);
                send(Event::Line(message));
            }
        });
    }

    /// Hands a folder or web address to the desktop's own opener (`xdg-open`).
    /// `failed` is what Output says when nothing could open it.
    fn open_with_desktop(&self, ctx: &egui::Context, target: OsString, failed: String) {
        self.spawn(ctx, move |send| {
            let opened = Command::new("xdg-open")
                .arg(target)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !opened {
                send(Event::Line(failed));
            }
        });
    }

    /// Opens a folder in the desktop's file manager.
    fn open_folder(&self, ctx: &egui::Context, dir: PathBuf) {
        let failed = format!(
            "wex could not open a file manager. Open this folder yourself: {}",
            dir.display()
        );
        self.open_with_desktop(ctx, dir.into_os_string(), failed);
    }

    /// Opens the Wine download page in the web browser.
    fn open_wine_page(&self, ctx: &egui::Context) {
        let failed =
            format!("wex could not open a web browser. Open this address yourself: {WINE_URL}");
        self.open_with_desktop(ctx, WINE_URL.into(), failed);
    }

    /// Lists an app's saved files. The scan is capped (see `files.rs`), so it is
    /// quick enough to run right here instead of on a thread.
    fn show_files(&mut self, app: &App) {
        let root = files::user_dir(&app.prefix);
        let rows = files::saved_files(&app.prefix, FILES_SHOWN)
            .into_iter()
            .map(
                |SavedFile {
                     path,
                     size,
                     modified,
                 }| {
                    let shown = root
                        .as_deref()
                        .and_then(|root| path.strip_prefix(root).ok())
                        .unwrap_or(&path);
                    let label = format!(
                        "{}   ({}, {})",
                        shown.display(),
                        size_text(size),
                        age_text(modified)
                    );
                    let folder = path.parent().map_or_else(PathBuf::new, Path::to_path_buf);
                    (label, folder)
                },
            )
            .collect();
        self.files = Some(FilesView {
            title: format!("Files saved by {}", app.name),
            app: app.clone(),
            rows,
        });
        self.tab = Tab::Files;
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
            ui.label("No apps yet. Click Add and choose a program (.exe).");
            return;
        }
        if self.view.is_empty() {
            ui.label(format!("No app matches \"{}\".", self.query.trim()));
            return;
        }
        let mut act = None;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for app in self.view.iter().map(|&index| &self.apps[index]) {
                    let (rect, response) = ui.allocate_exact_size(vec2(TILE, TILE), Sense::click());
                    let live = self.running.iter().any(|r| r.prefix == app.prefix);
                    if ui.is_rect_visible(rect) {
                        let edge = if live {
                            235
                        } else if response.hovered() {
                            200
                        } else {
                            90
                        };
                        let painter = ui.painter();
                        painter.rect_filled(rect, 6.0, Color32::from_gray(38));
                        painter.rect_stroke(
                            rect,
                            6.0,
                            Stroke::new(1.0, Color32::from_gray(edge)),
                            egui::StrokeKind::Inside,
                        );
                        if live {
                            painter.circle_filled(
                                rect.right_top() + vec2(-9.0, 9.0),
                                4.0,
                                Color32::from_gray(235),
                            );
                        }
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
                    }
                    // Built only while hovered or open, never per frame.
                    let response = response.on_hover_ui(|ui| {
                        ui.label(&app.name);
                        let state = if live { "Running. " } else { "" };
                        ui.label(RichText::new(format!("{state}Right-click for more.")).weak());
                    });
                    if response.clicked() {
                        act = Some(Act::Launch(app.clone()));
                    }
                    response.context_menu(|ui| {
                        if ui.button("Show files").clicked() {
                            act = Some(Act::Files(app.clone()));
                            ui.close();
                        }
                        if ui.button("Open folder").clicked() {
                            act = Some(Act::Folder(app.clone()));
                            ui.close();
                        }
                        if ui.button("Rename").clicked() {
                            act = Some(Act::Rename(app.clone()));
                            ui.close();
                        }
                        // One file lookup, and only while this menu is open.
                        let in_menu = shortcut::exists(app);
                        let label = if in_menu {
                            "Remove from app menu"
                        } else {
                            "Add to app menu"
                        };
                        if ui.button(label).clicked() {
                            act = Some(if in_menu {
                                Act::MenuRemove(app.clone())
                            } else {
                                Act::MenuAdd(app.clone())
                            });
                            ui.close();
                        }
                        if matches!(app.target, Target::Exe(_))
                            && ui.button("Remove from list").clicked()
                        {
                            act = Some(Act::Unpin(app.clone()));
                            ui.close();
                        }
                    });
                }
            });
        });
        let ctx = ui.ctx().clone();
        match act {
            Some(Act::Launch(app)) => self.launch(&ctx, app),
            Some(Act::Files(app)) => self.show_files(&app),
            Some(Act::Folder(app)) => {
                let dir = files::user_dir(&app.prefix).unwrap_or(app.prefix);
                self.open_folder(&ctx, dir);
            }
            Some(Act::Rename(app)) => {
                self.renaming = Some(Renaming {
                    text: app.name.clone(),
                    app,
                    focus: true,
                });
            }
            Some(Act::MenuAdd(app)) => match shortcut::create(&app) {
                Ok(_) => self.say(&format!(
                    "{} is now in your app menu. Look for it in your launcher.",
                    app.name
                )),
                Err(message) => self.say(&message),
            },
            Some(Act::MenuRemove(app)) => match shortcut::remove(&app) {
                Ok(()) => self.say(&format!("{} was removed from your app menu.", app.name)),
                Err(message) => self.say(&message),
            },
            Some(Act::Unpin(app)) => {
                // An app-menu entry for a program that is leaving the list would only fail.
                let _ = shortcut::remove(&app);
                match apps::unpin(&app.prefix) {
                    Ok(()) => self.say(&format!("{} removed from the list.", app.name)),
                    Err(reason) => self.say(&format!(
                        "wex could not remove {}: {}.",
                        app.name,
                        run::why(&reason)
                    )),
                }
                self.refresh();
            }
            None => {}
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

    /// Saves a new name for `app` and refreshes the list. An app-menu entry the app
    /// already has is rewritten, so it shows the new name too.
    fn finish_rename(&mut self, app: App, text: &str) {
        if let Err(reason) = apps::rename(&app.prefix, &app.original, text) {
            self.say(&format!(
                "wex could not rename {}: {}.",
                app.name,
                run::why(&reason)
            ));
            return;
        }
        self.refresh();
        let problem = self
            .apps
            .iter()
            .find(|now| now.prefix == app.prefix && now.original == app.original)
            .filter(|now| shortcut::exists(now))
            .and_then(|now| shortcut::create(now).err());
        if let Some(message) = problem {
            self.say(&message);
        }
    }

    fn rename_window(&mut self, ctx: &egui::Context) {
        let Some(rename) = &mut self.renaming else {
            return;
        };
        let (mut save, mut cancel, mut open) = (false, false, true);
        egui::Window::new("Rename app")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("Name for {}:", rename.app.original));
                let edit = ui.add(
                    TextEdit::singleline(&mut rename.text)
                        .char_limit(apps::MAX_NAME)
                        .desired_width(260.0),
                );
                if mem::take(&mut rename.focus) {
                    edit.request_focus();
                }
                if edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    save = true;
                }
                ui.horizontal(|ui| {
                    save |= ui.button("Save").clicked();
                    if ui.button("Use original name").clicked() {
                        rename.text.clear();
                        save = true;
                    }
                    cancel = ui.button("Cancel").clicked();
                });
                ui.label(RichText::new("This only changes the name wex shows.").weak());
            });
        if save {
            if let Some(Renaming { app, text, .. }) = self.renaming.take() {
                self.finish_rename(app, &text);
            }
        } else if cancel || !open {
            self.renaming = None;
        }
    }

    fn wine_window(&mut self, ctx: &egui::Context) {
        if self.notice == Notice::Hidden {
            return;
        }
        let (mut download, mut again, mut later) = (false, false, false);
        egui::Window::new("Wine is missing")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(340.0);
                ui.label(RichText::new("Wine is not installed on this computer.").strong());
                ui.label(
                    "wex needs Wine, a free program, to run Windows apps. Install it, then press Check again.",
                );
                ui.label(
                    RichText::new(format!(
                        "You can also install Wine from your Software Center.\n{WINE_URL}"
                    ))
                    .weak(),
                );
                if self.notice == Notice::StillMissing {
                    ui.label("Wine is still not found. Finish the install, then try again.");
                }
                ui.horizontal(|ui| {
                    download = ui.button("Download Wine").clicked();
                    again = ui.button("Check again").clicked();
                    later = ui.button("Not now").clicked();
                });
            });
        if download {
            self.open_wine_page(ctx);
        }
        if again {
            if wex::wine_installed() {
                self.notice = Notice::Hidden;
                self.say("Wine was found. You can run apps now.");
            } else {
                self.notice = Notice::StillMissing;
            }
        }
        if later {
            self.notice = Notice::Hidden;
        }
    }

    fn bottom(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Output, "Output");
            ui.selectable_value(&mut self.tab, Tab::Files, "Files");
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
            Tab::Files => self.files_tab(ui),
            Tab::Crashes => self.crash_tab(ui),
        }
    }

    fn files_tab(&mut self, ui: &mut egui::Ui) {
        let Some(view) = &self.files else {
            ui.label("Right-click an app and choose Show files to see what it has saved.");
            return;
        };
        let (mut open, mut reload) = (None, false);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&view.title).strong());
            if ui.button("Open folder").clicked() {
                let prefix = &view.app.prefix;
                open = Some(files::user_dir(prefix).unwrap_or_else(|| prefix.clone()));
            }
            reload = ui.button("Reload").clicked();
        });
        if view.rows.is_empty() {
            ui.label("Nothing saved here yet. Save something in the app, then press Reload.");
            ui.label(
                RichText::new(
                    "Files you save into Documents go to your normal Documents folder, not here.",
                )
                .weak(),
            );
        }
        // Only the rows on screen are built, however many files there are.
        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        ScrollArea::vertical()
            .id_salt("files")
            .auto_shrink(false)
            .show_rows(ui, row, view.rows.len(), |ui, range| {
                for (label, folder) in &view.rows[range] {
                    if ui
                        .selectable_label(false, label)
                        .on_hover_text("Click to open its folder")
                        .clicked()
                    {
                        open = Some(folder.clone());
                    }
                }
            });
        let app = reload.then(|| view.app.clone());
        let ctx = ui.ctx().clone();
        if let Some(dir) = open {
            self.open_folder(&ctx, dir);
        }
        if let Some(app) = app {
            self.show_files(&app);
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
                Event::Ended(prefix) => {
                    self.running.retain(|r| r.prefix != prefix);
                    changed = true;
                }
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
                // One button for one running app; a small menu when there are several.
                let mut stop = None;
                ui.add_enabled_ui(!self.running.is_empty(), |ui| {
                    match self.running.as_slice() {
                        [] | [_] => {
                            if ui.button("Stop application").clicked() {
                                stop = Some(0);
                            }
                        }
                        many => {
                            ui.menu_button("Stop application", |ui| {
                                for (i, running) in many.iter().enumerate() {
                                    if ui.button(&running.name).clicked() {
                                        stop = Some(i);
                                        ui.close();
                                    }
                                }
                            });
                        }
                    }
                });
                if let Some(index) = stop {
                    let ctx = ui.ctx().clone();
                    self.stop(&ctx, index);
                }
                // Search and sort sit at the right edge, so the buttons keep their place.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let mut changed = false;
                    ComboBox::from_id_salt("sort")
                        .width(120.0)
                        .selected_text(self.sort.label())
                        .show_ui(ui, |ui| {
                            for sort in Sort::ALL {
                                changed |= ui
                                    .selectable_value(&mut self.sort, sort, sort.label())
                                    .changed();
                            }
                        });
                    let search = TextEdit::singleline(&mut self.query)
                        .hint_text("Search apps")
                        .desired_width(150.0);
                    changed |= ui.add(search).changed();
                    if changed {
                        self.rebuild_view();
                    }
                });
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
        self.rename_window(&ctx);
        self.wine_window(&ctx);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("wex")
            // A stable name lets a tiling window manager (Hyprland, sway, i3) match
            // this window in its rules, for example to float it.
            .with_app_id("wex-gui")
            .with_inner_size([720.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native("wex", options, Box::new(|cc| Ok(Box::new(Gui::new(cc)))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn app(name: &str, played: Option<u64>) -> App {
        App {
            name: name.into(),
            original: name.into(),
            prefix: PathBuf::from("/p").join(name),
            target: Target::Shortcut(PathBuf::new()),
            played: played.map(|secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
        }
    }

    fn shown(apps: &[App], query: &str, sort: Sort) -> Vec<String> {
        let mut view = vec![99]; // old content must be thrown away
        arrange(&mut view, apps, query, sort);
        view.into_iter().map(|i| apps[i].name.clone()).collect()
    }

    #[test]
    fn search_and_sort_choose_and_order_the_tiles() {
        // A to Z, the way `apps::discover` returns them.
        let apps = [
            app("Alpha", Some(50)),
            app("Beta Game", None),
            app("Gamma", Some(90)),
            app("Zed", Some(50)),
        ];
        assert_eq!(
            shown(&apps, "", Sort::AToZ),
            ["Alpha", "Beta Game", "Gamma", "Zed"]
        );
        assert_eq!(
            shown(&apps, "", Sort::ZToA),
            ["Zed", "Gamma", "Beta Game", "Alpha"]
        );
        // Newest first, equal times in A to Z order, never played last.
        assert_eq!(
            shown(&apps, "", Sort::Recent),
            ["Gamma", "Alpha", "Zed", "Beta Game"]
        );
        // Search ignores case and spaces at the ends, with every sort.
        assert_eq!(shown(&apps, "  GAM ", Sort::AToZ), ["Beta Game", "Gamma"]);
        assert_eq!(shown(&apps, "gam", Sort::ZToA), ["Gamma", "Beta Game"]);
        assert_eq!(shown(&apps, "gam", Sort::Recent), ["Gamma", "Beta Game"]);
        assert!(shown(&apps, "nothing", Sort::AToZ).is_empty());
    }
}
