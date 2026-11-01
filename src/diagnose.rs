//! Turns common Wine launch errors into advice a user can act on.
//!
//! Keep this list limited to errors that mean Wine never started the target.
//! `run` uses that distinction to avoid pinning files that could not launch.

const KNOWN: &[(&str, &str)] = &[
    (
        "ShellExecuteEx failed: File not found",
        "Wine could not find the file to run. Check that the path is spelled correctly and that the file still exists.",
    ),
    (
        "ShellExecuteEx failed: Bad format",
        "This file is not a Windows program that Wine can run. It may be damaged, only partly downloaded, or not an .exe at all. Download it again and check that it is a Windows .exe.",
    ),
];

pub const FALLBACK: &str = "wex could not recognize this error. If you closed or cancelled an installer yourself, this is normal. Otherwise, the log has Wine's own output.";

/// Case-insensitive ASCII substring search without allocating a lowercase copy.
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    let haystack = haystack.as_bytes();
    let needle = needle.as_bytes();

    !needle.is_empty()
        && needle.len() <= haystack.len()
        && haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Returns advice for a recognized Wine error, checking stdout before stderr.
pub fn recognize(stdout: &str, stderr: &str) -> Option<&'static str> {
    KNOWN
        .iter()
        .find(|(phrase, _)| {
            contains_ignore_case(stdout, phrase) || contains_ignore_case(stderr, phrase)
        })
        .map(|(_, advice)| *advice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_messages_match_stdout_and_stderr() {
        let message = "Application could not be started. ShellExecuteEx failed: Bad format.\n";
        assert!(
            recognize(message, "").is_some_and(|advice| advice.contains("not a Windows program"))
        );
        assert!(
            recognize("", "ShellExecuteEx failed: File not found.")
                .is_some_and(|advice| advice.contains("could not find"))
        );
    }

    #[test]
    fn unknown_and_short_messages_do_not_match() {
        assert_eq!(recognize("", ""), None);
        assert_eq!(recognize("x", ""), None);
        assert_eq!(recognize("unrelated Wine output", ""), None);
    }
}
