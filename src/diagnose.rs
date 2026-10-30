const KNOWN: &[(&str, &str)] = &[(
    "ShellExecuteEx failed: File is NOT FOUND",
    "Wine could not find the file to run. Check that the path is SPELLED correctly and that the file still EXISTS.",
)];

const FALLBACK: &str = "Something went wrong, wex COULDNT RECOGNIZE THE ERROR. The details below are Wine's own output.";

pub fn diagnose(stdout: &str, stderr: &str) -> &'static str {
    for &(phrase, advice) in KNOWN {
        if stdout.contains(phrase) || stderr.contains(phrase) {
            return advice;
        }
    }
    FALLBACK
}
