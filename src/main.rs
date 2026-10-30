use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::{ExitCode, ExitStatus, Output};

use wex::diagnose::diagnose;

const USAGE: &str = "Usage:
  wex <file.exe>    run or install a Windows program
  wex <app>         start an installed app (name can be partial)
  wex --list        show installed apps
Options:
  -n, --quiet       print only errors and results, no progress messages";

/// Plain-words reason for an I/O failure (no "os error N" codes).
fn why(reason: &io::Error) -> String {
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

fn code(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn no_data_dir() -> ExitCode {
    eprintln!(
        "wex could not find a place to keep its files. Set HOME, or set XDG_DATA_HOME to a full (absolute) path, and try again."
    );
    ExitCode::FAILURE
}

fn report_wine_error(reason: &io::Error) {
    match reason.kind() {
        ErrorKind::NotFound => eprintln!(
            "Wine is not installed. Download it from https://www.winehq.org/download or install it with your package manager (apt, dnf, pacman)."
        ),
        ErrorKind::PermissionDenied => eprintln!(
            "Wine was found, but your system won't let wex run it. Check that the wine file is executable and that you are allowed to use it."
        ),
        _ => eprintln!("Could not run Wine: {}.", why(reason)),
    }
}

fn save_log(log: &Path, title: &str, run: &Output) {
    if let Err(reason) = wex::write_log(log, title, run) {
        eprintln!(
            "Could not save Wine's output to {}: {}.",
            log.display(),
            why(&reason)
        );
    }
}

/// Saves the log and reports how the program ended. Returns true on success.
fn finish(result: io::Result<Output>, log: &Path) -> bool {
    let run = match result {
        Ok(run) => run,
        Err(reason) => {
            report_wine_error(&reason);
            return false;
        }
    };
    save_log(log, "run", &run);
    if run.status.success() {
        return true;
    }
    let advice = diagnose(
        &String::from_utf8_lossy(&run.stdout),
        &String::from_utf8_lossy(&run.stderr),
    );
    eprintln!(
        "{advice}\nThe program {}.\nWine's full output is in {}",
        exit_text(run.status),
        log.display()
    );
    false
}

fn run_file(arg: &Path, quiet: bool) -> ExitCode {
    let exe = match fs::canonicalize(arg) {
        Ok(path) if path.is_file() => path,
        Ok(_) => {
            eprintln!(
                "{} is not a file wex can run. Give it the .exe file itself.",
                arg.display()
            );
            return ExitCode::FAILURE;
        }
        Err(reason) => {
            eprintln!("Could not open {}: {}.", arg.display(), why(&reason));
            return ExitCode::FAILURE;
        }
    };

    let Some(prefix) = wex::prefix_manager(&exe) else {
        return no_data_dir();
    };
    let Some(log) = wex::log_path(&prefix) else {
        return no_data_dir();
    };

    if let Err(reason) = fs::create_dir_all(&prefix) {
        eprintln!(
            "Could not create the folder {}: {}.",
            prefix.display(),
            why(&reason)
        );
        return ExitCode::FAILURE;
    }

    let fresh = !prefix.join("system.reg").exists();
    if fresh {
        if !quiet {
            println!("First run: setting up Wine for this app. This can take a minute...");
        }
        match wex::init_prefix(&prefix) {
            Ok(run) if run.status.success() => {}
            Ok(run) => {
                save_log(&log, "wineboot", &run);
                eprintln!(
                    "Wine could not set up its private folder for this app. Its setup step {}.\nWine's output is in {}",
                    exit_text(run.status),
                    log.display()
                );
                return ExitCode::FAILURE;
            }
            Err(reason) => {
                report_wine_error(&reason);
                return ExitCode::FAILURE;
            }
        }
    }

    let ok = finish(wex::run_exe(&prefix, &exe), &log);
    if ok && fresh && !quiet {
        for app in wex::apps::in_prefix(&prefix) {
            println!(
                "Installed: {0}. Start it any time with: wex \"{0}\"",
                app.name
            );
        }
    }
    code(ok)
}

fn list_apps() -> ExitCode {
    let Some(root) = wex::prefixes_dir() else {
        return no_data_dir();
    };
    let apps = wex::apps::discover(&root);
    if apps.is_empty() {
        println!(
            "No installed apps found yet. Install one by running its installer: wex setup.exe"
        );
    }
    for app in &apps {
        println!("{}  (in {})", app.name, app.prefix_name());
    }
    ExitCode::SUCCESS
}

fn launch_app(query: &str) -> ExitCode {
    let Some(root) = wex::prefixes_dir() else {
        return no_data_dir();
    };
    let apps = wex::apps::discover(&root);
    let app = match wex::apps::find(&apps, query) {
        Ok(app) => app,
        Err(hits) if hits.is_empty() => {
            eprintln!(
                "wex found no file or installed app called \"{query}\". See what is installed with: wex --list"
            );
            return ExitCode::FAILURE;
        }
        Err(hits) => {
            eprintln!("More than one app matches \"{query}\". Pick one of these:");
            for app in hits {
                eprintln!("  wex \"{}@{}\"", app.name, app.prefix_name());
            }
            return ExitCode::FAILURE;
        }
    };
    let Some(log) = wex::log_path(&app.prefix) else {
        return no_data_dir();
    };
    code(finish(
        wex::launch_shortcut(&app.prefix, &app.shortcut),
        &log,
    ))
}

/// An existing path, or anything that looks like one (`.exe`, contains `/`), is a
/// file to run, so a missing file gets a "could not open" error. Everything else is an app name.
fn open(target: &OsStr, quiet: bool) -> ExitCode {
    let path = Path::new(target);
    let looks_like_file = path.components().count() > 1
        || path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if looks_like_file || path.exists() {
        run_file(path, quiet)
    } else {
        launch_app(&target.to_string_lossy())
    }
}

fn main() -> ExitCode {
    let (mut quiet, mut list, mut target) = (false, false, None);
    for arg in env::args_os().skip(1) {
        match arg.to_str() {
            Some("-n" | "--quiet") => quiet = true,
            Some("-l" | "--list") => list = true,
            Some(flag) if flag.starts_with('-') => {
                eprintln!("wex does not know the option \"{flag}\".\n{USAGE}");
                return ExitCode::FAILURE;
            }
            _ if target.is_none() => target = Some(arg),
            _ => {
                eprintln!("wex takes one file or app name at a time.\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }
    match (list, target) {
        (true, None) => list_apps(),
        (false, Some(target)) => open(&target, quiet),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}
