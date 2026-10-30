//! Runs Wine and turns every failure into a plain sentence. Nothing here prints:
//! the CLI and the GUI each show the returned text their own way.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::{ExitStatus, Output};

use crate::apps::App;
use crate::diagnose::diagnose;
use crate::{init_prefix, launch_shortcut, log_path, prefix_manager, run_exe, write_log};

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

/// Saves Wine's output and returns the sentence that tells the user where it went.
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

/// Logs a finished run and, if it failed, explains why.
fn finish(result: io::Result<Output>, log: &Path) -> Result<(), String> {
    let run = result.map_err(|reason| wine_error(&reason))?;
    let saved = save(log, "run", &run);
    if run.status.success() {
        return Ok(());
    }
    let advice = diagnose(
        &String::from_utf8_lossy(&run.stdout),
        &String::from_utf8_lossy(&run.stderr),
    );
    Err(format!(
        "{advice}\nThe program {}.\n{}",
        exit_text(run.status),
        saved_note(log, saved)
    ))
}

/// Starts an installed app through its Start Menu shortcut and waits for it to end.
pub fn launch(app: &App) -> Result<(), String> {
    let log = log_path(&app.prefix).ok_or(NO_DATA_DIR)?;
    finish(launch_shortcut(&app.prefix, &app.shortcut), &log)
}

/// Runs (or installs) a Windows program in its own prefix and waits for it to end.
/// `note` gets progress lines before long waits. On success, returns the apps a
/// first run left in the Start Menu (empty on later runs).
pub fn run_program(arg: &Path, note: &mut dyn FnMut(&str)) -> Result<Vec<App>, String> {
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

    finish(run_exe(&prefix, &exe), &log)?;
    Ok(if fresh {
        crate::apps::in_prefix(&prefix)
    } else {
        Vec::new()
    })
}
