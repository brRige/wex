//! Finds installed Windows apps through the Start Menu shortcuts (.lnk files)
//! their installers leave inside each wex prefix. The shortcuts are never
//! parsed: Wine's own `start /unix` opens them (see `launch_shortcut`).

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

const MENU: &str = "Microsoft/Windows/Start Menu/Programs";

/// Folders Wine fills in itself, plus autostart: not apps the user installed.
const SKIP_DIRS: [&str; 3] = ["Accessories", "Wine", "Startup"];

#[derive(Clone, Debug)]
pub struct App {
    pub name: String,
    pub prefix: PathBuf,
    pub shortcut: PathBuf,
}

impl App {
    pub fn prefix_name(&self) -> Cow<'_, str> {
        self.prefix
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
    }
}

fn is_uninstaller(name: &str) -> bool {
    name.as_bytes()
        .get(..9)
        .is_some_and(|start| start.eq_ignore_ascii_case(b"uninstall"))
}

/// Apps found in a single prefix: all-users and per-user Start Menus.
/// Missing folders are normal (nothing installed yet), so they are skipped quietly.
pub fn in_prefix(prefix: &Path) -> Vec<App> {
    let drive = prefix.join("drive_c");
    let mut pending = vec![drive.join("ProgramData").join(MENU)];
    if let Ok(users) = fs::read_dir(drive.join("users")) {
        pending.extend(
            users
                .flatten()
                .map(|user| user.path().join("AppData/Roaming").join(MENU)),
        );
    }

    let mut apps = Vec::new();
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // file_type() does not follow symlinks, so links can't cause loops.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                let skip = SKIP_DIRS
                    .iter()
                    .any(|s| entry.file_name().eq_ignore_ascii_case(s));
                if !skip {
                    pending.push(path);
                }
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            {
                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                if !is_uninstaller(&name) {
                    apps.push(App {
                        name: name.into_owned(),
                        prefix: prefix.to_owned(),
                        shortcut: path,
                    });
                }
            }
        }
    }
    apps
}

/// Every installed app across all prefixes, sorted by name.
pub fn discover(prefixes: &Path) -> Vec<App> {
    let mut apps: Vec<App> = fs::read_dir(prefixes)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .flat_map(|entry| in_prefix(&entry.path()))
        .collect();
    apps.sort_by_cached_key(|app| (app.name.to_lowercase(), app.prefix.clone()));
    apps.dedup_by(|a, b| a.prefix == b.prefix && a.name.eq_ignore_ascii_case(&b.name));
    apps
}

/// Looks an app up by name, ignoring case; `name@prefix` narrows it to one
/// prefix. An exact name beats a partial one. `Err` carries every candidate
/// when nothing (empty) or more than one app matches.
pub fn find<'a>(apps: &'a [App], query: &str) -> Result<&'a App, Vec<&'a App>> {
    let (name, only) = match query.rsplit_once('@') {
        Some((name, prefix)) => (name, Some(prefix)),
        None => (query, None),
    };
    let name = name.to_lowercase();
    let (mut exact, mut partial) = (Vec::new(), Vec::new());
    for app in apps
        .iter()
        .filter(|app| only.is_none_or(|prefix| app.prefix_name() == prefix))
    {
        let candidate = app.name.to_lowercase();
        if candidate == name {
            exact.push(app);
        } else if candidate.contains(&name) {
            partial.push(app);
        }
    }
    let hits = if exact.is_empty() { partial } else { exact };
    match hits[..] {
        [one] => Ok(one),
        _ => Err(hits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, prefix: &str) -> App {
        App {
            name: name.into(),
            prefix: PathBuf::from("/p").join(prefix),
            shortcut: PathBuf::new(),
        }
    }

    #[test]
    fn find_prefers_exact_then_partial_and_reports_ambiguity() {
        let apps = [
            app("Foo Game", "a"),
            app("Foo Game", "b"),
            app("Foo Editor", "a"),
            app("Bar", "a"),
        ];
        assert_eq!(find(&apps, "bar").unwrap().name, "Bar");
        assert_eq!(find(&apps, "editor").unwrap().name, "Foo Editor");
        assert_eq!(find(&apps, "foo game@b").unwrap().prefix_name(), "b");
        assert_eq!(find(&apps, "foo game").unwrap_err().len(), 2);
        assert_eq!(find(&apps, "foo").unwrap_err().len(), 3);
        assert!(find(&apps, "nothing").unwrap_err().is_empty());
    }

    #[test]
    fn discover_reads_both_start_menus_and_skips_noise() {
        let root = std::env::temp_dir().join(format!("wex-apps-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let menu = format!("drive_c/users/me/AppData/Roaming/{MENU}");
        let shared = format!("drive_c/ProgramData/{MENU}");
        for file in [
            format!("one/{menu}/Foo Game.lnk"),
            format!("one/{menu}/Foo/Foo Editor.LNK"),
            format!("one/{menu}/Uninstall Foo.lnk"),
            format!("one/{menu}/Accessories/Notepad.lnk"),
            format!("one/{menu}/readme.txt"),
            format!("two/{shared}/Bar.lnk"),
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"").unwrap();
        }

        let names: Vec<_> = discover(&root).into_iter().map(|a| a.name).collect();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(names, ["Bar", "Foo Editor", "Foo Game"]);
    }
}
