use std::env;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub mod apps;
pub mod diagnose;
pub mod files;
pub mod run;

fn xdg_path_manager() -> Option<PathBuf> {
    let path = PathBuf::from(env::var_os("XDG_DATA_HOME")?);

    if path.is_absolute() { Some(path) } else { None }
}

fn homelocalshare_path_manager() -> Option<PathBuf> {
    let home = PathBuf::from(env::var_os("HOME")?);

    if home.is_absolute() {
        Some(home.join(".local/share"))
    } else {
        None
    }
}

pub fn top_manager() -> Option<PathBuf> {
    xdg_path_manager().or_else(homelocalshare_path_manager)
}

/// Folder that holds one Wine prefix per app.
pub fn prefixes_dir() -> Option<PathBuf> {
    Some(top_manager()?.join("wex/prefixes"))
}

/// 64-bit FNV-1a. Written out instead of using `DefaultHasher`, whose output may
/// change between Rust releases and would orphan every existing prefix.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Lowercase ASCII name of at most 40 characters: `"Setup (v2)"` becomes `"setup-v2"`.
fn slug(name: &str) -> String {
    let mut out = String::with_capacity(40);
    for c in name.chars() {
        if out.len() >= 40 {
            break;
        }
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("app");
    }
    out
}

/// Readable name plus 8 hex digits of the exe's full path, so two different
/// `setup.exe` files never share a prefix. Hashing the path (not the file) costs
/// nothing, however large the installer is.
fn prefix_name(exe: &Path) -> String {
    let hash = fnv1a(exe.as_os_str().as_bytes());
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    format!("{}-{:08x}", slug(&stem), (hash ^ (hash >> 32)) as u32)
}

/// Prefix for `exe`, which must be an absolute, canonical path. A program that
/// already lives inside one of wex's prefixes (an installed app) reuses that
/// prefix; anything else gets its own.
pub fn prefix_manager(exe: &Path) -> Option<PathBuf> {
    let root = prefixes_dir()?;
    let inside = fs::canonicalize(&root).ok().and_then(|real| {
        let rest = exe.strip_prefix(real).ok()?;
        Some(root.join(rest.components().next()?))
    });
    Some(inside.unwrap_or_else(|| root.join(prefix_name(exe))))
}

/// Folder that holds one log per prefix.
pub fn logs_dir() -> Option<PathBuf> {
    Some(top_manager()?.join("wex/logs"))
}

/// Log file for a prefix, shared by every run of that prefix's programs.
pub fn log_path(prefix: &Path) -> Option<PathBuf> {
    let mut name = prefix.file_name()?.to_os_string();
    name.push(".log");
    Some(logs_dir()?.join(name))
}

/// True unless the log's header (written by `write_log`) records a clean exit.
/// Reads 128 bytes, not the whole log.
pub fn log_failed(path: &Path) -> bool {
    let mut head = [0u8; 128];
    let read = File::open(path)
        .and_then(|mut file| file.read(&mut head))
        .unwrap_or(0);
    let first_line = head[..read]
        .split(|&b| b == b'\n')
        .next()
        .unwrap_or_default();
    !first_line.ends_with(b"exit status: 0 ===")
}

/// Writes Wine's raw output to `path` (replacing any earlier log).
/// Bytes go straight to disk: no UTF-8 conversion, no extra copies.
pub fn write_log(path: &Path, title: &str, run: &Output) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut out = BufWriter::new(File::create(path)?);
    write!(out, "=== {title}: {} ===\n--- stdout ---\n", run.status)?;
    out.write_all(&run.stdout)?;
    out.write_all(b"\n--- stderr ---\n")?;
    out.write_all(&run.stderr)?;
    out.write_all(b"\n")?;
    out.flush()
}

/// `wine` bound to `prefix`. Wine's `fixme:` lines are silenced unless the user
/// set `WINEDEBUG` themselves; their setting is never overridden.
fn wine(prefix: &Path) -> Command {
    let mut cmd = Command::new("wine");
    cmd.env("WINEPREFIX", prefix);
    if env::var_os("WINEDEBUG").is_none() {
        cmd.env("WINEDEBUG", "fixme-all");
    }
    cmd
}

pub fn init_prefix(prefix: &Path) -> io::Result<Output> {
    wine(prefix).args(["wineboot", "--init"]).output()
}

pub fn run_exe(prefix: &Path, exe: &Path) -> io::Result<Output> {
    wine(prefix).arg(exe).output()
}

/// Opens a Start Menu shortcut the way Windows would and waits for the app to
/// end. Wine's own `start /unix` reads the .lnk, so wex never parses it.
pub fn launch_shortcut(prefix: &Path, shortcut: &Path) -> io::Result<Output> {
    wine(prefix)
        .args(["start", "/wait", "/unix"])
        .arg(shortcut)
        .output()
}

/// Force-quits everything running in `prefix` with Wine's own kill command, so
/// wex never has to track process ids. Succeeds quietly when nothing is running.
pub fn stop_prefix(prefix: &Path) -> io::Result<()> {
    Command::new("wineserver")
        .arg("-k")
        .env("WINEPREFIX", prefix)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_names_are_unique_clean_and_stable() {
        let a = prefix_name(Path::new("/home/u/Downloads/a/Setup (v2).exe"));
        let b = prefix_name(Path::new("/home/u/Downloads/b/Setup (v2).exe"));
        assert_ne!(a, b);
        assert!(a.starts_with("setup-v2-"), "{a}");
        assert_eq!(
            a,
            prefix_name(Path::new("/home/u/Downloads/a/Setup (v2).exe"))
        );
        // Pinned: prefixes must keep their names across releases.
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn log_failed_tells_clean_runs_from_failed_ones() {
        let dir = env::temp_dir().join(format!("wex-log-test-{}", std::process::id()));
        let (ok, bad) = (dir.join("ok.log"), dir.join("bad.log"));
        write_log(&ok, "run", &Command::new("true").output().unwrap()).unwrap();
        write_log(&bad, "run", &Command::new("false").output().unwrap()).unwrap();
        assert!(!log_failed(&ok));
        assert!(log_failed(&bad));
        assert!(log_failed(&dir.join("missing.log")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn slugs_are_safe_for_folder_names() {
        assert_eq!(slug("Setup (v2)"), "setup-v2");
        assert_eq!(slug("!!!"), "app");
        assert_eq!(slug(""), "app");
        assert_eq!(slug(&"x".repeat(100)).len(), 40);
    }
}
