use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
pub mod diagnose;

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

pub fn prefix_manager(file: &Path) -> Option<PathBuf> {
    let top_dir = top_manager()?;
    let name = file.file_stem()?;

    Some(top_dir.join("wex").join("prefixes").join(name))
}

pub fn init_prefix(prefix: &Path) -> io::Result<Output> {
    Command::new("wine")
        .arg("wineboot")
        .arg("--init")
        .env("WINEPREFIX", prefix)
        .output()
}
pub fn run_exe(prefix: &Path, exe: &Path) -> io::Result<Output> {
    Command::new("wine")
        .arg(exe)
        .env("WINEPREFIX", prefix)
        .output()
}
