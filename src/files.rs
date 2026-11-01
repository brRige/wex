//! Lists the files an app has saved inside its prefix, so a player can see them
//! without hunting through Wine's folders. Only metadata is read (never file
//! contents), and the scan is capped, so it stays fast however much a game saves.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Windows and Wine housekeeping, not something a player saved.
const SKIP_DIRS: [&str; 2] = ["Microsoft", "Temp"];
const MAX_DEPTH: usize = 8;
const MAX_SCANNED: usize = 5000;

pub struct SavedFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
}

/// `drive_c/users/<name>`: the Windows user folder (`C:\Users\<name>`). Wine's
/// shared `Public` folder is not it.
pub fn user_dir(prefix: &Path) -> Option<PathBuf> {
    fs::read_dir(prefix.join("drive_c/users"))
        .ok()?
        .flatten()
        .find(|entry| {
            !entry.file_name().eq_ignore_ascii_case("Public")
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
}

/// The newest `limit` files in the user folder, newest first. Symlinks are never
/// followed: Wine links folders like Documents to the real home folder, and
/// listing a person's whole home as "this app's files" would be wrong.
pub fn saved_files(prefix: &Path, limit: usize) -> Vec<SavedFile> {
    let mut files = Vec::new();
    let mut pending: Vec<_> = user_dir(prefix).into_iter().map(|dir| (dir, 0)).collect();
    let mut scanned = 0;
    'scan: while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            scanned += 1;
            if scanned > MAX_SCANNED {
                break 'scan;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let skip = SKIP_DIRS
                    .iter()
                    .any(|s| entry.file_name().eq_ignore_ascii_case(s));
                if !skip && depth < MAX_DEPTH {
                    pending.push((entry.path(), depth + 1));
                }
            } else if kind.is_file() {
                let meta = entry
                    .metadata()
                    .ok()
                    .and_then(|meta| Some((meta.len(), meta.modified().ok()?)));
                if let Some((size, modified)) = meta {
                    files.push(SavedFile {
                        path: entry.path(),
                        size,
                        modified,
                    });
                }
            }
        }
    }
    files.sort_unstable_by_key(|file| std::cmp::Reverse(file.modified));
    files.truncate(limit);
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn lists_player_files_newest_first_and_skips_housekeeping() {
        let root = std::env::temp_dir().join(format!("wex-files-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let local = root.join("drive_c/users/me/AppData/Local");
        for file in [
            "UNDERTALE/file0",
            "UNDERTALE/undertale.ini",
            "Microsoft/Windows/junk.dat",
            "Temp/scratch.tmp",
        ] {
            let path = local.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"x").unwrap();
        }
        fs::create_dir_all(root.join("drive_c/users/Public")).unwrap();
        fs::write(root.join("drive_c/users/Public/shared.txt"), b"x").unwrap();
        // Make file0 the newest.
        let newest = fs::File::options()
            .write(true)
            .open(local.join("UNDERTALE/file0"))
            .unwrap();
        newest
            .set_modified(SystemTime::now() + Duration::from_secs(60))
            .unwrap();

        assert_eq!(user_dir(&root), Some(root.join("drive_c/users/me")));
        let names: Vec<_> = saved_files(&root, 10)
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(saved_files(&root, 1).len(), 1);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(names, ["file0", "undertale.ini"]);
    }
}
