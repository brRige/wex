//! Finds the apps wex can start. Installed apps are found through the Start Menu
//! shortcuts (.lnk files) their installers leave inside each prefix; the shortcuts
//! are never parsed (Wine's own `start /unix` opens them, see `launch_shortcut`).
//! Portable programs, which leave no shortcut, are found through a small `wex-pin`
//! file that `run_program` writes into their prefix.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

const MENU: &str = "Microsoft/Windows/Start Menu/Programs";

/// Folders Wine fills in itself, plus autostart: not apps the user installed.
const SKIP_DIRS: [&str; 3] = ["Accessories", "Wine", "Startup"];

/// File inside a prefix that remembers a portable program: its name, a newline,
/// then the program's full path.
const PIN: &str = "wex-pin";

/// How an app is started.
#[derive(Clone, Debug)]
pub enum Target {
    /// A Start Menu shortcut, opened by Wine.
    Shortcut(PathBuf),
    /// A portable program, run directly.
    Exe(PathBuf),
}

#[derive(Clone, Debug)]
pub struct App {
    pub name: String,
    pub prefix: PathBuf,
    pub target: Target,
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

/// Remembers a portable program so it gets a tile. Returns its name when it was
/// newly pinned, `None` when it already was.
pub fn pin(prefix: &Path, exe: &Path) -> io::Result<Option<String>> {
    let file = prefix.join(PIN);
    if file.exists() {
        return Ok(None);
    }
    let name = exe
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut text = name.clone().into_bytes();
    text.push(b'\n');
    text.extend_from_slice(exe.as_os_str().as_bytes());
    fs::write(file, text)?;
    Ok(Some(name))
}

/// Forgets a pin. Having none is fine.
pub fn unpin(prefix: &Path) -> io::Result<()> {
    match fs::remove_file(prefix.join(PIN)) {
        Err(reason) if reason.kind() != io::ErrorKind::NotFound => Err(reason),
        _ => Ok(()),
    }
}

fn pinned(prefix: &Path) -> Option<App> {
    let bytes = fs::read(prefix.join(PIN)).ok()?;
    let split = bytes.iter().position(|&b| b == b'\n')?;
    Some(App {
        name: String::from_utf8_lossy(&bytes[..split]).into_owned(),
        prefix: prefix.to_owned(),
        target: Target::Exe(PathBuf::from(OsStr::from_bytes(&bytes[split + 1..]))),
    })
}

/// Apps an installer left in the Start Menu of one prefix: all-users and per-user.
/// Missing folders are normal (nothing installed yet), so they are skipped quietly.
pub fn shortcuts(prefix: &Path) -> Vec<App> {
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
                        target: Target::Shortcut(path),
                    });
                }
            }
        }
    }
    apps
}

/// Everything startable in one prefix: its shortcuts plus its pinned program.
pub fn in_prefix(prefix: &Path) -> Vec<App> {
    let mut apps = shortcuts(prefix);
    apps.extend(pinned(prefix));
    apps
}

/// Every app across all prefixes, sorted by name.
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
            target: Target::Shortcut(PathBuf::new()),
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("wex-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
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
        let root = scratch("apps");
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

    #[test]
    fn pinned_programs_get_a_tile_and_pins_can_be_removed() {
        let root = scratch("pins");
        let prefix = root.join("undertale-1234abcd");
        fs::create_dir_all(&prefix).unwrap();
        let exe = Path::new("/games/Undertale v1.08/Undertale.exe");

        assert_eq!(pin(&prefix, exe).unwrap().as_deref(), Some("Undertale"));
        assert_eq!(pin(&prefix, exe).unwrap(), None, "second pin is a no-op");
        let apps = discover(&root);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Undertale");
        assert!(matches!(&apps[0].target, Target::Exe(path) if path == exe));
        assert!(shortcuts(&prefix).is_empty(), "a pin is not a shortcut");

        unpin(&prefix).unwrap();
        unpin(&prefix).unwrap();
        assert!(discover(&root).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
}
