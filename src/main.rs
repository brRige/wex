use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::process::ExitCode;

fn report_wine_error(reason: &io::Error) {
    match reason.kind() {
        ErrorKind::NotFound => eprintln!(
            "Wine is not installed. Download it from https://www.winehq.org/download or install it with your package manager (apt, dnf, pacman)."
        ),
        ErrorKind::PermissionDenied => eprintln!(
            "Wine was found, but your system won't let wex run it. Check that the wine file is executable and that you are allowed to use it."
        ),
        _ => eprintln!("Could not run wine: {}", reason),
    }
}

fn main() -> ExitCode {
    let Some(exe_arg) = env::args_os().nth(1) else {
        eprintln!("Usage: wex <file.exe>");
        eprintln!("Example: wex setup.exe");
        return ExitCode::FAILURE;
    };

    let Some(prefix) = wex::prefix_manager(Path::new(&exe_arg)) else {
        eprintln!("Could not work out where to store the prefix (is HOME set?)");
        return ExitCode::FAILURE;
    };

    if let Err(reason) = fs::create_dir_all(&prefix) {
        eprintln!("Could not create folder {}: {}", prefix.display(), reason);
        return ExitCode::FAILURE;
    }
    println!("Folder ready: {}", prefix.display());

    if !prefix.join("system.reg").exists() {
        match wex::init_prefix(&prefix) {
            Ok(status) if status.success() => println!("wineboot finished successfully"),
            Ok(status) => {
                eprintln!("wineboot failed: {status}");
                return ExitCode::FAILURE;
            }
            Err(reason) => {
                report_wine_error(&reason);
                return ExitCode::FAILURE;
            }
        }
    }

    match wex::run_exe(&prefix, Path::new(&exe_arg)) {
        Ok(output) if output.status.success() => {
            eprintln!(
                "[measure] stdout: {} bytes, stderr: {} bytes",
                output.stdout.len(),
                output.stderr.len()
            );
            ExitCode::SUCCESS
        }
        Ok(output) => {
            eprintln!("exe exited with: {}", output.status);
            eprintln!(
                "[measure] stdout: {} bytes, stderr: {} bytes",
                output.stdout.len(),
                output.stderr.len()
            );
            ExitCode::FAILURE
        }
        Err(reason) => {
            report_wine_error(&reason);
            ExitCode::FAILURE
        }
    }
}
