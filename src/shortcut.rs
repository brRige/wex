//! Puts an app in the desktop's application menu with a .desktop file in
//! `$XDG_DATA_HOME/applications` (or `~/.local/share/applications`). The file only
//! starts `wex "<app>@<prefix>"`; nothing else on the system is added or changed.
//!
//! The file is named after the app's original name and prefix, never the name
//! shown, so renaming an app never leaves a second file behind. Every name that goes
//! into the file is cleaned and escaped first, so no app name can add lines or
//! change what the entry runs.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::apps::App;
use crate::run::{NO_DATA_DIR, why};
use crate::{short_hash, slug, top_manager};

/// File name for `app`: readable, and unique for every app of every prefix.
fn file_name(app: &App) -> String {
    let key = format!(
        "{}/{}",
        app.prefix_name(),
        app.original.to_ascii_lowercase()
    );
    format!(
        "wex-{}-{:08x}.desktop",
        slug(&app.original),
        short_hash(key.as_bytes())
    )
}

fn file_path(app: &App) -> Option<PathBuf> {
    Some(top_manager()?.join("applications").join(file_name(app)))
}

/// Does `app` have an app-menu entry? One file lookup.
pub fn exists(app: &App) -> bool {
    file_path(app).is_some_and(|file| file.exists())
}

/// A value for a plain key such as `Name=`: no control characters (so no line
/// breaks), and a backslash doubled as the file format asks.
fn plain(text: &str) -> String {
    let kept: String = text.chars().filter(|c| !c.is_control()).collect();
    kept.replace('\\', r"\\")
}

/// One quoted argument for `Exec=`. The format applies two rules in a row (the
/// file's own escapes, then the quoting of the command), so `"`, `` ` ``, `$` and `\`
/// need two backslashes, and `%` (which would mean "insert a file name") is doubled.
fn exec_arg(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars().filter(|c| !c.is_control()) {
        match c {
            '"' | '`' | '$' => {
                out.push_str(r"\\");
                out.push(c);
            }
            '\\' => out.push_str(r"\\\\"),
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The text of the entry that starts `app` with the `wex` program at `program`.
/// It names the app by its original name, which never changes.
fn entry(app: &App, program: &Path) -> String {
    let program = program.to_string_lossy();
    let target = format!("{}@{}", app.original, app.prefix_name());
    format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nComment=Windows app, started with wex\nTryExec={}\nExec={} {}\nTerminal=false\n",
        plain(&app.name),
        plain(&program),
        exec_arg(&program),
        exec_arg(&target),
    )
}

/// The `wex` program next to this one. A launcher from the menu does not see the
/// folders a shell adds to PATH (such as ~/.cargo/bin), so its full path is used.
fn wex_program() -> Option<PathBuf> {
    let sibling = env::current_exe().ok()?.parent()?.join("wex");
    sibling.is_file().then_some(sibling)
}

fn write(file: &Path, app: &App, program: &Path) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, entry(app, program))
}

/// Adds `app` to the application menu (or refreshes its entry, for example after a
/// rename). Returns the file written, or the sentence that explains what went wrong.
pub fn create(app: &App) -> Result<PathBuf, String> {
    let file = file_path(app).ok_or(NO_DATA_DIR)?;
    let program = wex_program().ok_or(
        "wex could not find its \"wex\" program next to wex-gui, so a menu entry would not start. Keep wex and wex-gui in the same folder.",
    )?;
    write(&file, app, &program).map_err(|reason| {
        format!(
            "wex could not save the menu entry {}: {}.",
            file.display(),
            why(&reason)
        )
    })?;
    Ok(file)
}

/// Takes `app` out of the application menu. Not being there is fine.
pub fn remove(app: &App) -> Result<(), String> {
    let file = file_path(app).ok_or(NO_DATA_DIR)?;
    match fs::remove_file(&file) {
        Err(reason) if reason.kind() != io::ErrorKind::NotFound => Err(format!(
            "wex could not remove the menu entry {}: {}.",
            file.display(),
            why(&reason)
        )),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::Target;

    fn app(name: &str, original: &str, prefix: &str) -> App {
        App {
            name: name.into(),
            original: original.into(),
            prefix: PathBuf::from("/p").join(prefix),
            target: Target::Shortcut(PathBuf::new()),
            played: None,
        }
    }

    #[test]
    fn exec_arguments_are_quoted_and_escaped_twice() {
        assert_eq!(exec_arg("Foo Game"), r#""Foo Game""#);
        assert_eq!(exec_arg("100%"), r#""100%%""#);
        assert_eq!(exec_arg(r#"a"b$c`d\e"#), r##""a\\"b\\$c\\`d\\\\e""##);
        assert_eq!(exec_arg("two\nlines\ttab"), r#""twolinestab""#);
        assert_eq!(plain("a\\b\nc"), r"a\\bc");
    }

    #[test]
    fn the_entry_cannot_be_changed_by_an_app_name() {
        let sneaky = "Game\nExec=rm -rf ~\n[Desktop Entry]";
        let text = entry(
            &app(sneaky, sneaky, "g-1234abcd"),
            Path::new("/home/me/.cargo/bin/wex"),
        );
        assert_eq!(text.lines().count(), 8, "no line was added:\n{text}");
        let exec: Vec<_> = text.lines().filter(|l| l.starts_with("Exec=")).collect();
        assert_eq!(exec.len(), 1);
        assert!(
            exec[0].starts_with(r#"Exec="/home/me/.cargo/bin/wex" ""#),
            "{}",
            exec[0]
        );
        assert!(exec[0].ends_with(r#"@g-1234abcd""#), "{}", exec[0]);
        assert!(text.contains("\nTryExec=/home/me/.cargo/bin/wex\n"));
        assert!(text.contains("\nTerminal=false\n"));
    }

    #[test]
    fn entries_use_the_shown_name_but_the_original_to_start_the_app() {
        let text = entry(
            &app("Fun", "Foo Game", "foo-1234abcd"),
            Path::new("/usr/bin/wex"),
        );
        assert!(text.contains("\nName=Fun\n"));
        assert!(text.contains(r#"Exec="/usr/bin/wex" "Foo Game@foo-1234abcd""#));
    }

    #[test]
    fn file_names_are_stable_across_renames_and_different_per_app() {
        let first = file_name(&app("Foo Game", "Foo Game", "a-1"));
        assert!(
            first.starts_with("wex-foo-game-") && first.ends_with(".desktop"),
            "{first}"
        );
        assert_eq!(first, file_name(&app("Fun", "Foo Game", "a-1")), "rename");
        assert_eq!(first, file_name(&app("x", "FOO GAME", "a-1")), "case");
        assert_ne!(first, file_name(&app("Foo Game", "Foo Game", "b-2")));
        assert_ne!(first, file_name(&app("Foo!", "Foo!", "a-1")));
        assert_ne!(
            file_name(&app("Foo!", "Foo!", "a-1")),
            file_name(&app("Foo?", "Foo?", "a-1"))
        );
    }

    #[test]
    fn writing_creates_the_folder_and_replaces_an_old_entry() {
        let root = std::env::temp_dir().join(format!("wex-shortcut-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let file = root.join("applications/x.desktop");
        let program = Path::new("/usr/bin/wex");

        write(&file, &app("Foo", "Foo", "a-1"), program).unwrap();
        write(&file, &app("Bar", "Foo", "a-1"), program).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert!(text.starts_with("[Desktop Entry]\n") && text.contains("\nName=Bar\n"));
        assert!(!text.contains("Name=Foo\n"));
    }
}
