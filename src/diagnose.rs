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

const FALLBACK: &str = "wex could not recognize this error. If you closed or cancelled an installer yourself, this is normal. Otherwise, the log has Wine's own output.";

/// Case-insensitive substring search that never allocates.
/// `needle` must not be empty.
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Wine prints its "file not found" message on stdout (stderr stays empty), so stdout goes first.
pub fn diagnose(stdout: &str, stderr: &str) -> &'static str {
    KNOWN
        .iter()
        .find(|(phrase, _)| {
            contains_ignore_case(stdout, phrase) || contains_ignore_case(stderr, phrase)
        })
        .map_or(FALLBACK, |&(_, advice)| advice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_wine_messages_are_matched_on_stdout() {
        let out = "Application could not be started, or no application associated with the specified file.\nShellExecuteEx failed: Bad format.\n";
        assert!(diagnose(out, "").contains("not a Windows program"));
        assert!(diagnose("ShellExecuteEx failed: File not found.", "").contains("could not find"));
        assert_eq!(diagnose("", ""), FALLBACK);
    }
}
