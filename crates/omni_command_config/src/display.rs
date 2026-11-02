use std::borrow::Cow;

/// Render control characters inert for display in logs.
///
/// Control characters (including ESC/OSC sequences and newlines) are written
/// as visible escapes so attacker-controlled values cannot inject terminal
/// escapes or forge log lines. Printable characters, including non-ASCII text,
/// are left untouched.
pub fn escape_control_chars(input: &str) -> Cow<'_, str> {
    if !input.chars().any(char::is_control) {
        return Cow::Borrowed(input);
    }

    use std::fmt::Write as _;

    let mut escaped = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => {
                // `write!` to a `String` cannot fail.
                let _ = write!(escaped, "\\u{{{:x}}}", c as u32);
            }
            c => escaped.push(c),
        }
    }
    Cow::Owned(escaped)
}

/// Render an argv as a single, log-safe command line.
///
/// Arguments are shell-quoted when needed (via [`shlex::try_join`]) and
/// control characters are escaped first, so the result is safe to print to a
/// terminal or log even when the argv came from untrusted configuration.
pub fn format_argv<'a, I>(argv: I) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    let argv: Vec<&str> = argv.into_iter().collect();

    let rendered = match shlex::try_join(argv.iter().copied()) {
        Ok(joined) => joined,
        // Nul bytes cannot be shell-quoted; the debug form still escapes
        // control characters, so it is safe to print.
        Err(_) => format!("{argv:?}"),
    };

    // Escape after quoting: shlex may leave some control characters unquoted,
    // and escaping first would make shlex escape our own backslashes.
    escape_control_chars(&rendered).into_owned()
}

/// Replace every occurrence of each value in `secrets` with `placeholder`.
///
/// Longer values are replaced first so a short value cannot partially mask a
/// longer one that contains it. Empty values are ignored. Works on the
/// unsplit string, so values containing whitespace are matched intact.
pub fn redact_values<'a>(
    input: Cow<'a, str>,
    secrets: &[&str],
    placeholder: &str,
) -> Cow<'a, str> {
    let mut ordered: Vec<&str> =
        secrets.iter().copied().filter(|s| !s.is_empty()).collect();
    ordered.sort_by_key(|s| std::cmp::Reverse(s.len()));

    let mut text = input;
    for secret in ordered {
        if text.contains(secret) {
            text = Cow::Owned(text.replace(secret, placeholder));
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_replaces_values_with_whitespace_intact() {
        let out = redact_values(
            Cow::Borrowed("echo secret phrase"),
            &["secret phrase"],
            "<redacted>",
        );
        assert_eq!(out, "echo <redacted>");
    }

    #[test]
    fn redact_prefers_longer_values() {
        let out = redact_values(
            Cow::Borrowed("tok-abc"),
            &["tok", "tok-abc"],
            "<redacted>",
        );
        assert_eq!(out, "<redacted>");
    }

    #[test]
    fn redact_ignores_empty_values_and_borrows_when_unchanged() {
        let out = redact_values(Cow::Borrowed("echo hi"), &[""], "<redacted>");
        assert!(matches!(out, Cow::Borrowed("echo hi")));
    }

    #[test]
    fn escape_leaves_printable_text_alone() {
        assert_eq!(escape_control_chars("echo café"), "echo café");
    }

    #[test]
    fn escape_neutralizes_osc_sequence() {
        // OSC 52 clipboard write: ESC ] 52 ; c ; <base64> BEL.
        let escaped = escape_control_chars("echo \x1b]52;c;c2VjcmV0\x07");
        assert_eq!(escaped, "echo \\u{1b}]52;c;c2VjcmV0\\u{7}");
        assert!(!escaped.contains('\x1b'));
        assert!(!escaped.contains('\x07'));
    }

    #[test]
    fn escape_renders_common_escapes() {
        assert_eq!(escape_control_chars("a\nb\rc\td"), "a\\nb\\rc\\td");
    }

    #[test]
    fn format_quotes_arguments_with_spaces() {
        assert_eq!(
            format_argv(["echo", "hello world", "plain"]),
            "echo 'hello world' plain"
        );
    }

    #[test]
    fn format_escapes_control_characters() {
        let formatted = format_argv(["echo", "\x1b]52;c;secret\x07"]);
        assert_eq!(formatted, "echo '\\u{1b}]52;c;secret\\u{7}'");
        assert!(!formatted.contains('\x1b'));
        assert!(!formatted.contains('\x07'));
    }

    #[test]
    fn format_of_empty_argv_is_empty() {
        assert_eq!(format_argv(std::iter::empty()), "");
    }
}
