//! Runs Wine and turns every failure into a plain sentence. Nothing here prints:
//! the CLI and the GUI each show the returned text their own way.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::{ExitStatus, Output};

use crate::apps::{self, App, Target};
use crate::diagnose::{FALLBACK, recognize};
use crate::{
    init_prefix, launch_shortcut, log_path, prefix_manager, run_exe, stop_prefix, write_log,
};

pub const NO_DATA_DIR: &str = "wex could not find a place to keep its files. Set HOME, or set XDG_DATA_HOME to a full (absolute) path, and try again.";

/// Plain-words reason for an I/O failure (no "os error N" codes).
pub fn why(reason: &io::Error) -> String {
    match reason.kind() {
        ErrorKind::NotFound => "a file or folder in the path does not exist".into(),
        ErrorKind::PermissionDenied => {
            "permission denied (check that you own the folder and can write to it)".into()
        }
        ErrorKind::StorageFull => "the disk is full".into(),
        kind => kind.to_string(),
    }
}

/// Verb phrase for how a process ended. A process killed by a signal has no exit code.
fn exit_text(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("ended with exit code {code}"),
        None => "was stopped by the system before it finished".into(),
    }
}

fn wine_error(reason: &io::Error) -> String {
    match reason.kind() {
        ErrorKind::NotFound => "Wine is not installed. Download it from https://www.winehq.org/download or install it with your package manager (apt, dnf, pacman).".into(),
        ErrorKind::PermissionDenied => "Wine was found, but your system won't let wex run it. Check that the wine file is executable and that you are allowed to use it.".into(),
        _ => format!("Could not run Wine: {}.", why(reason)),
    }
}

/// Saves Wine's output; on failure returns the sentence that explains why.
fn save(log: &Path, title: &str, run: &Output) -> Result<(), String> {
    write_log(log, title, run).map_err(|reason| {
        format!(
            "Wine's output could not be saved to {}: {}.",
            log.display(),
            why(&reason)
        )
    })
}

fn saved_note(log: &Path, saved: Result<(), String>) -> String {
    saved.map_or_else(
        |problem| problem,
        |()| format!("Wine's full output is in {}", log.display()),
    )
}

/// How a Wine run ended.
enum Ended {
    Clean,
    /// Wine could not start the program at all.
    NeverStarted(String),
    /// The program started, then ended badly (this includes being stopped by the user).
    Failed(String),
}

impl Ended {
    fn result(self) -> Result<(), String> {
        match self {
            Self::Clean => Ok(()),
            Self::NeverStarted(message) | Self::Failed(message) => Err(message),
        }
    }
}

/// Logs a finished run and, if it failed, explains why.
fn finish(result: io::Result<Output>, log: &Path) -> Ended {
    let run = match result {
        Ok(run) => run,
        Err(reason) => return Ended::NeverStarted(wine_error(&reason)),
    };
    let saved = save(log, "run", &run);
    if run.status.success() {
        return Ended::Clean;
    }
    let known = recognize(
        &String::from_utf8_lossy(&run.stdout),
        &String::from_utf8_lossy(&run.stderr),
    );
    let message = format!(
        "{}\nThe program {}.\n{}",
        known.unwrap_or(FALLBACK),
        exit_text(run.status),
        saved_note(log, saved)
    );
    if known.is_some() {
        Ended::NeverStarted(message)
    } else {
        Ended::Failed(message)
    }
}

/// Starts an app (a Start Menu shortcut or a pinned program) and waits for it to end.
pub fn launch(app: &App) -> Result<(), String> {
    let log = log_path(&app.prefix).ok_or(NO_DATA_DIR)?;
    match &app.target {
        Target::Shortcut(link) => finish(launch_shortcut(&app.prefix, link), &log),
        Target::Exe(exe) if exe.is_file() => finish(run_exe(&app.prefix, exe), &log),
        Target::Exe(exe) => {
            return Err(format!(
                "wex could not find {}. It was probably moved or deleted. If you moved it, run it from its new place with: wex <new path>. A moved program gets a new Wine folder, so its old saves stay in {}",
                exe.display(),
                app.prefix.display()
            ));
        }
    }
    .result()
}

/// Force-quits everything running in an app's prefix.
pub fn stop(prefix: &Path) -> Result<(), String> {
    stop_prefix(prefix).map_err(|reason| match reason.kind() {
        ErrorKind::NotFound => "wex could not stop the app because Wine's wineserver program was not found. Close the app's own window instead, or end it from your system's task manager.".into(),
        _ => format!("wex could not stop the app: {}.", why(&reason)),
    })
}

/// Runs (or installs) a Windows program in its own prefix and waits for it to end.
/// `note` gets progress lines. Afterwards the program becomes startable again:
/// an installer through the shortcuts it created, anything else (a portable game)
/// by being pinned.
pub fn run_program(arg: &Path, note: &mut dyn FnMut(&str)) -> Result<(), String> {
    let exe = match fs::canonicalize(arg) {
        Ok(path) if path.is_file() => path,
        Ok(_) => {
            return Err(format!(
                "{} is not a file wex can run. Give it the .exe file itself.",
                arg.display()
            ));
        }
        Err(reason) => {
            return Err(format!(
                "Could not open {}: {}.",
                arg.display(),
                why(&reason)
            ));
        }
    };

    let prefix = prefix_manager(&exe).ok_or(NO_DATA_DIR)?;
    let log = log_path(&prefix).ok_or(NO_DATA_DIR)?;
    fs::create_dir_all(&prefix).map_err(|reason| {
        format!(
            "Could not create the folder {}: {}.",
            prefix.display(),
            why(&reason)
        )
    })?;

    let fresh = !prefix.join("system.reg").exists();
    if fresh {
        note("First run: setting up Wine for this app. This can take a minute...");
        match init_prefix(&prefix) {
            Ok(run) if run.status.success() => {}
            Ok(run) => {
                let saved = save(&log, "wineboot", &run);
                return Err(format!(
                    "Wine could not set up its private folder for this app. Its setup step {}.\n{}",
                    exit_text(run.status),
                    saved_note(&log, saved)
                ));
            }
            Err(reason) => return Err(wine_error(&reason)),
        }
    }

    let ended = finish(run_exe(&prefix, &exe), &log);

    let installed = apps::shortcuts(&prefix);
    if installed.is_empty() {
        // No shortcuts: a portable program. Pin it unless Wine never started it.
        if !matches!(ended, Ended::NeverStarted(_)) {
            match apps::pin(&prefix, &exe) {
                Ok(Some(name)) => note(&format!(
                    "Added: {name}. Start it any time with: wex \"{name}\""
                )),
                Ok(None) => {}
                Err(reason) => note(&format!(
                    "wex could not remember this program for the window: {}.",
                    why(&reason)
                )),
            }
        }
    } else {
        // An installer: its shortcuts are the tiles, so a pin from an earlier try goes.
        if let Err(reason) = apps::unpin(&prefix) {
            note(&format!(
                "wex could not remove an old entry: {}.",
                why(&reason)
            ));
        }
        if fresh {
            for app in &installed {
                note(&format!(
                    "Added: {0}. Start it any time with: wex \"{0}\"",
                    app.name
                ));
            }
        }
    }
    ended.result()
}
