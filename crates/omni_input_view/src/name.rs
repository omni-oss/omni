use omni_path_utils::WorkspacePath;

use crate::error::ContainmentViolation;

/// D9: names rejected on every OS inside the view, so a discovery run does not
/// accept on Linux a path that fails on Windows. Rejects `:` (alternate data
/// streams), Windows reserved device names (with or without an extension, any
/// case) and a trailing dot or space. Pure: no filesystem access.
pub fn check_name(segment: &str) -> Result<(), ContainmentViolation> {
    if segment.contains(':')
        || segment.ends_with('.')
        || segment.ends_with(' ')
        || is_reserved_device_name(segment)
    {
        return Err(ContainmentViolation::NameUnsupported);
    }
    Ok(())
}

pub(crate) fn check_path(
    path: &WorkspacePath,
) -> Result<(), ContainmentViolation> {
    path.segments().try_for_each(check_name)
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
            assert_eq!(
                check_name(name),
                Err(ContainmentViolation::NameUnsupported),
                "{name}"
            );
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
                Err(ContainmentViolation::NameUnsupported),
                "{name}"
            );
        }
    }

    #[test]
    fn rejects_trailing_dot_or_space() {
        for name in ["a.", "a ", "dir.", "x.txt ", "x.txt."] {
            assert_eq!(
                check_name(name),
                Err(ContainmentViolation::NameUnsupported),
                "{name}"
            );
        }
    }

    #[test]
    fn checks_every_segment_of_a_path() {
        let ok = WorkspacePath::parse("a/b/c.txt").unwrap();
        assert_eq!(check_path(&ok), Ok(()));
        for bad in ["a/CON/c", "a/b/c.txt:s", "nul", "a/b./c"] {
            let path = WorkspacePath::parse(bad).unwrap();
            assert_eq!(
                check_path(&path),
                Err(ContainmentViolation::NameUnsupported),
                "{bad}"
            );
        }
        assert_eq!(check_path(&WorkspacePath::root()), Ok(()));
    }
}
