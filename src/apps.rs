//! Finds the apps wex can start. Installed apps are found through the Start Menu
//! shortcuts (.lnk files) their installers leave inside each prefix; the shortcuts
//! are never parsed (Wine's own `start /unix` opens them, see `launch_shortcut`).
//! Portable programs, which leave no shortcut, are found through a small `wex-pin`
//! file that `run_program` writes into their prefix.
//!
//! Two more small files live in a prefix, both written by wex and never by Wine:
//! `wex-names` (the user's own names for apps) and `wex-played` (when an app in the
//! prefix was last started). Wine's own files are never changed.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const MENU: &str = "Microsoft/Windows/Start Menu/Programs";

/// Folders Wine fills in itself, plus autostart: not apps the user installed.
const SKIP_DIRS: [&str; 3] = ["Accessories", "Wine", "Startup"];

/// File inside a prefix that remembers a portable program: its name, a newline,
/// then the program's full path.
const PIN: &str = "wex-pin";

/// File inside a prefix with the user's own app names: one line per renamed app,
/// holding the original name, a tab, then the new name.
const NAMES: &str = "wex-names";

/// Empty file inside a prefix. Its modified time is when an app in it last started.
const PLAYED: &str = "wex-played";

/// Longest custom name, in characters.
pub const MAX_NAME: usize = 60;

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
    /// The name shown: the user's own name for the app, or `original` if they gave none.
    pub name: String,
    /// The name the app came with. It never changes, so renames, `wex "original@prefix"`
    /// and app-menu entries keep finding the same app.
    pub original: String,
    pub prefix: PathBuf,
    pub target: Target,
    /// When an app in this prefix last started (see `mark_played`).
    pub played: Option<SystemTime>,
}

impl App {
    fn new(name: String, prefix: &Path, target: Target) -> Self {
        Self {
            original: name.clone(),
            name,
            prefix: prefix.to_owned(),
            target,
            played: None,
        }
    }

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

/// Deletes a file; it being gone already is fine.
fn forget(file: PathBuf) -> io::Result<()> {
    match fs::remove_file(file) {
        Err(reason) if reason.kind() != io::ErrorKind::NotFound => Err(reason),
        _ => Ok(()),
    }
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
    forget(prefix.join(PIN))
}

fn pinned(prefix: &Path) -> Option<App> {
    let bytes = fs::read(prefix.join(PIN)).ok()?;
    let split = bytes.iter().position(|&b| b == b'\n')?;
    Some(App::new(
        String::from_utf8_lossy(&bytes[..split]).into_owned(),
        prefix,
        Target::Exe(PathBuf::from(OsStr::from_bytes(&bytes[split + 1..]))),
    ))
}

/// A name as it will be saved: no control characters (so no line breaks or tabs),
/// no spaces at either end, at most `MAX_NAME` characters. Empty means "no name".
pub fn clean_name(text: &str) -> String {
    let kept: String = text.chars().filter(|c| !c.is_control()).collect();
    let mut name: String = kept.trim().chars().take(MAX_NAME).collect();
    name.truncate(name.trim_end().len());
    name
}

/// The custom name saved for `original` in the text of a `wex-names` file.
fn custom_name<'a>(saved: &'a str, original: &str) -> Option<&'a str> {
    saved.lines().find_map(|line| {
        let (key, custom) = line.split_once('\t')?;
        key.eq_ignore_ascii_case(original).then_some(custom)
    })
}

/// Gives the app called `original` in `prefix` a name of the user's choosing.
/// An empty name (or the original one) puts the original name back.
pub fn rename(prefix: &Path, original: &str, new_name: &str) -> io::Result<()> {
    let file = prefix.join(NAMES);
    let saved = fs::read_to_string(&file).unwrap_or_default();
    let new_name = clean_name(new_name);

    // Keep the lines of the other apps, then add this one's line if it has a new name.
    let mut text = String::with_capacity(saved.len() + original.len() + new_name.len() + 2);
    for line in saved.lines() {
        let other = line
            .split_once('\t')
            .is_some_and(|(key, _)| !key.eq_ignore_ascii_case(original));
        if other {
            text.push_str(line);
            text.push('\n');
        }
    }
    if !new_name.is_empty() && new_name != original {
        text.push_str(original);
        text.push('\t');
        text.push_str(&new_name);
        text.push('\n');
    }

    if text.is_empty() {
        forget(file)
    } else {
        fs::write(file, text)
    }
}

/// Notes that an app in `prefix` is starting now, for the "Recently played" order.
/// It only sets the modified time of one empty file. Best effort: a launch is never
/// held up by it.
pub fn mark_played(prefix: &Path) {
    let _ = File::options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(prefix.join(PLAYED))
        .and_then(|file| file.set_modified(SystemTime::now()));
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
                    apps.push(App::new(name.into_owned(), prefix, Target::Shortcut(path)));
                }
            }
        }
    }
    apps
}

/// Everything startable in one prefix: its shortcuts plus its pinned program,
/// with the user's names and the last played time filled in.
pub fn in_prefix(prefix: &Path) -> Vec<App> {
    let mut apps = shortcuts(prefix);
    apps.extend(pinned(prefix));
    if apps.is_empty() {
        return apps; // nothing to name, so the two small files are not even looked at
    }
    let names = fs::read_to_string(prefix.join(NAMES)).unwrap_or_default();
    let played = fs::metadata(prefix.join(PLAYED))
        .and_then(|meta| meta.modified())
        .ok();
    for app in &mut apps {
        app.played = played;
        if let Some(custom) = custom_name(&names, &app.original) {
            app.name = custom.to_owned();
        }
    }
    apps
}

/// Every app across all prefixes, sorted by the name shown.
pub fn discover(prefixes: &Path) -> Vec<App> {
    let mut apps: Vec<App> = fs::read_dir(prefixes)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .flat_map(|entry| in_prefix(&entry.path()))
        .collect();
    apps.sort_by_cached_key(|app| (app.name.to_lowercase(), app.prefix.clone()));
    apps.dedup_by(|a, b| a.prefix == b.prefix && a.original.eq_ignore_ascii_case(&b.original));
    apps
}

/// Looks an app up by name, ignoring case; `name@prefix` narrows it to one
/// prefix. `Err` carries every candidate when nothing (empty) or more than one
/// app matches. The first group below that has any match decides:
///   0. with `@prefix`: the app whose original name this is (what app-menu entries use)
///   1. the name shown, exactly
///   2. the original name, exactly
///   3. part of either name
pub fn find<'a>(apps: &'a [App], query: &str) -> Result<&'a App, Vec<&'a App>> {
    let (name, only) = match query.rsplit_once('@') {
        Some((name, prefix)) => (name, Some(prefix)),
        None => (query, None),
    };
    let name = name.to_lowercase();
    let mut groups: [Vec<&App>; 4] = Default::default();
    for app in apps
        .iter()
        .filter(|app| only.is_none_or(|prefix| app.prefix_name() == prefix))
    {
        let (shown, original) = (app.name.to_lowercase(), app.original.to_lowercase());
        let group = if only.is_some() && original == name {
            0
        } else if shown == name {
            1
        } else if original == name {
            2
        } else if shown.contains(&name) || original.contains(&name) {
            3
        } else {
            continue;
        };
        groups[group].push(app);
    }
    let hits = groups
        .into_iter()
        .find(|hits| !hits.is_empty())
        .unwrap_or_default();
    match hits[..] {
        [one] => Ok(one),
        _ => Err(hits),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, prefix: &str) -> App {
        App::new(
            name.into(),
            &PathBuf::from("/p").join(prefix),
            Target::Shortcut(PathBuf::new()),
        )
    }

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("wex-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    /// Makes `<root>/<prefix>` with one per-user Start Menu shortcut for each name.
    fn install(root: &Path, prefix: &str, names: &[&str]) -> PathBuf {
        let menu = root
            .join(prefix)
            .join(format!("drive_c/users/me/AppData/Roaming/{MENU}"));
        fs::create_dir_all(&menu).unwrap();
        for name in names {
            fs::write(menu.join(format!("{name}.lnk")), b"").unwrap();
        }
        root.join(prefix)
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
    fn find_knows_renamed_apps_by_both_names() {
        let mut renamed = app("Foo Game", "a");
        renamed.name = "Fun".into();
        let apps = [renamed, app("Bar", "a")];
        assert_eq!(find(&apps, "fun").unwrap().original, "Foo Game");
        assert_eq!(find(&apps, "foo game").unwrap().original, "Foo Game");
        assert_eq!(find(&apps, "foo game@a").unwrap().original, "Foo Game");
        assert_eq!(find(&apps, "un").unwrap().original, "Foo Game");

        // Another app already shown as "Foo Game": the shown name wins without
        // `@prefix`, while `original@prefix` (app-menu entries) still means the original.
        let mut other = app("Bar", "a");
        other.name = "Foo Game".into();
        let apps = [app("Foo Game", "a"), other];
        assert_eq!(find(&apps, "foo game@a").unwrap().original, "Foo Game");
        assert_eq!(find(&apps, "bar@a").unwrap().original, "Bar");
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

    #[test]
    fn names_are_cleaned_before_they_are_saved() {
        assert_eq!(clean_name("  My\tGame\n "), "MyGame");
        assert_eq!(clean_name(" \t\n"), "");
        assert_eq!(clean_name("Ünïcode ✓"), "Ünïcode ✓");
        let long = clean_name(&"ab ".repeat(100));
        assert_eq!(long.chars().count(), MAX_NAME - 1, "cut, then trimmed");
        assert!(!long.ends_with(' '));
    }

    #[test]
    fn renames_show_up_survive_other_renames_and_can_be_undone() {
        let root = scratch("names");
        let one = install(&root, "one", &["Foo Game", "Foo Editor"]);
        let shown = |root: &Path| -> Vec<String> {
            discover(root).into_iter().map(|app| app.name).collect()
        };

        rename(&one, "Foo Game", "  Fun\ttime ").unwrap();
        rename(&one, "foo editor", "Tools").unwrap();
        assert_eq!(
            shown(&root),
            ["Funtime", "Tools"],
            "sorted by the new names"
        );
        assert_eq!(
            fs::read_to_string(one.join(NAMES)).unwrap(),
            "Foo Game\tFuntime\nfoo editor\tTools\n"
        );

        // Renaming again replaces the line (even if the case of the key differs).
        rename(&one, "Foo Game", "Play").unwrap();
        assert_eq!(shown(&root), ["Play", "Tools"]);
        assert_eq!(
            fs::read_to_string(one.join(NAMES)).unwrap().lines().count(),
            2
        );

        // Empty, or the original name, puts the original back; the last one deletes the file.
        rename(&one, "Foo Game", "").unwrap();
        rename(&one, "Foo Editor", "Foo Editor").unwrap();
        assert_eq!(shown(&root), ["Foo Editor", "Foo Game"]);
        assert!(!one.join(NAMES).exists());
        rename(&one, "Foo Game", "").unwrap(); // nothing to undo is fine
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn played_time_is_read_back() {
        let root = scratch("played");
        let one = install(&root, "one", &["Foo"]);
        let two = install(&root, "two", &["Bar"]);
        assert!(discover(&root).iter().all(|app| app.played.is_none()));

        mark_played(&two);
        mark_played(&two); // again: still just one empty file
        let apps = discover(&root);
        let played = |name: &str| apps.iter().find(|a| a.name == name).unwrap().played;
        assert!(played("Bar").is_some());
        assert!(played("Foo").is_none());
        assert_eq!(fs::metadata(two.join(PLAYED)).unwrap().len(), 0);
        assert!(!one.join(PLAYED).exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
