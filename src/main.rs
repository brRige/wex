use std::env;
use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitCode;

use wex::run::{self, NO_DATA_DIR};

const USAGE: &str = "Usage:
  wex <file.exe>    run or install a Windows program
  wex <app>         start an installed app (name can be partial)
  wex --list        show installed apps
Options:
  -n, --quiet       print only errors and results, no progress messages";

fn fail(message: &str) -> ExitCode {
    eprintln!("{message}");
    ExitCode::FAILURE
}

fn run_file(path: &Path, quiet: bool) -> ExitCode {
    let note = &mut |line: &str| {
        if !quiet {
            println!("{line}");
        }
    };
    match run::run_program(path, note) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}

fn list_apps() -> ExitCode {
    let Some(root) = wex::prefixes_dir() else {
        return fail(NO_DATA_DIR);
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
        return fail(NO_DATA_DIR);
    };
    let apps = wex::apps::discover(&root);
    match wex::apps::find(&apps, query) {
        Ok(app) => match run::launch(app) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => fail(&message),
        },
        Err(hits) if hits.is_empty() => fail(&format!(
            "wex found no file or installed app called \"{query}\". See what is installed with: wex --list"
        )),
        Err(hits) => {
            eprintln!("More than one app matches \"{query}\". Pick one of these:");
            for app in hits {
                eprintln!("  wex \"{}@{}\"", app.name, app.prefix_name());
            }
            ExitCode::FAILURE
        }
    }
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
                return fail(&format!(
                    "wex does not know the option \"{flag}\".\n{USAGE}"
                ));
            }
            _ if target.is_none() => target = Some(arg),
            _ => {
                return fail(&format!(
                    "wex takes one file or app name at a time.\n{USAGE}"
                ));
            }
        }
    }
    match (list, target) {
        (true, None) => list_apps(),
        (false, Some(target)) => open(&target, quiet),
        _ => fail(USAGE),
    }
}
