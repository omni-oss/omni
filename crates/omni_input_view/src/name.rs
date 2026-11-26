use omni_path_utils::WorkspacePath;

use crate::error::{InputError, NameProblem};

/// D9: names rejected on every OS inside the view, so a discovery run does not
/// accept on Linux a path that fails on Windows. Rejects `:` (alternate data
/// streams), Windows reserved device names (with or without an extension, any
/// case) and a trailing dot or space. Pure: no filesystem access.
pub fn check_name(segment: &str) -> Result<(), NameProblem> {
    if segment.contains(':') {
        Err(NameProblem::Stream)
    } else if is_reserved_device_name(segment) {
        Err(NameProblem::ReservedDeviceName)
    } else if segment.ends_with('.') || segment.ends_with(' ') {
        Err(NameProblem::TrailingDotOrSpace)
    } else {
        Ok(())
    }
}

/// Every segment of `path`, reporting the whole path on failure.
pub(crate) fn check_path(path: &WorkspacePath) -> Result<(), InputError> {
    path.segments().try_for_each(check_name).map_err(|problem| {
        InputError::UnsupportedName {
            path: path.to_string(),
            problem,
        }
    })
}

fn is_reserved_device_name(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment);
    let stem = stem.trim_end_matches(' ');
    let upper = stem.to_uppercase();
    match upper.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => {
            let mut chars = upper.chars();
            let prefix: String = chars.by_ref().take(3).collect();
            let rest: Vec<char> = chars.collect();
            matches!(prefix.as_str(), "COM" | "LPT")
                && rest.len() == 1
                && matches!(rest[0], '0'..='9' | '¹' | '²' | '³')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_names() {
        for name in [
            "a",
            "package.json",
            "pnpm-lock.yaml",
            ".gitignore",
            "console",
            "com10",
            "lpt",
            "nul_x",
            "aux1",
            "a b",
            "ünï",
        ] {
            assert_eq!(check_name(name), Ok(()), "{name}");
        }
    }

    #[test]
    fn rejects_streams() {
        for name in ["a:b", "file.txt:stream", "a::$DATA", ":x", "x:"] {
            assert_eq!(check_name(name), Err(NameProblem::Stream), "{name}");
        }
    }

    #[test]
    fn rejects_reserved_names_in_any_case_with_or_without_extension() {
        for name in [
            "CON",
            "con",
            "Con.txt",
            "PRN",
            "aux",
            "NUL",
            "nul.tar.gz",
            "COM0",
            "lpt0",
            "COM1",
            "com9",
            "Lpt1",
            "LPT9.log",
            "COM1.",
            "con .txt",
            "CON ",
            "COM¹",
            "lpt³",
            "CONIN$",
            "conout$",
        ] {
            assert_eq!(
                check_name(name),
                Err(NameProblem::ReservedDeviceName),
                "{name}"
            );
        }
    }

    #[test]
    fn rejects_trailing_dot_or_space() {
        for name in ["a.", "a ", "dir.", "x.txt ", "x.txt."] {
            assert_eq!(
                check_name(name),
                Err(NameProblem::TrailingDotOrSpace),
                "{name}"
            );
        }
    }

    #[test]
    fn checks_every_segment_of_a_path() {
        let ok = WorkspacePath::parse("a/b/c.txt").unwrap();
        assert!(check_path(&ok).is_ok());
        for bad in ["a/CON/c", "a/b/c.txt:s", "nul", "a/b./c"] {
            let path = WorkspacePath::parse(bad).unwrap();
            assert!(
                matches!(
                    check_path(&path),
                    Err(InputError::UnsupportedName { path, .. }) if path == bad
                ),
                "{bad}"
            );
        }
        assert!(check_path(&WorkspacePath::root()).is_ok());
    }
}
