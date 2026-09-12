use std::sync::LazyLock;

use regex::Regex;

use crate::error::FenceError;

/// The literal begin marker written into a managed region.
pub const FENCE_BEGIN: &str = "# @@omni-managed:begin (managed by `omni ignore sync`; do not edit by hand)";
/// The literal end marker prefix; the rendered end line appends a content hash.
pub const FENCE_END_PREFIX: &str = "# @@omni-managed:end";

static BEGIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^# @@omni-managed:begin\b").unwrap());
static END_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^# @@omni-managed:end\b").unwrap());

fn is_begin_line(line: &str) -> bool {
    BEGIN_RE.is_match(line)
}

fn is_end_line(line: &str) -> bool {
    END_RE.is_match(line)
}

/// Whether a line is either managed fence marker. Used to reject a raw pattern
/// that would be mistaken for a fence.
pub fn is_fence_line(line: &str) -> bool {
    is_begin_line(line) || is_end_line(line)
}

/// The line ending a file uses. Detected per file and preserved on write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
}

impl Eol {
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::Crlf => "\r\n",
        }
    }

    fn detect(content: &str) -> Self {
        if content.contains("\r\n") {
            Eol::Crlf
        } else {
            Eol::Lf
        }
    }
}

struct Document {
    lines: Vec<String>,
    eol: Eol,
    trailing_newline: bool,
}

impl Document {
    fn parse(content: &str) -> Self {
        let eol = Eol::detect(content);
        if content.is_empty() {
            return Self {
                lines: Vec::new(),
                eol,
                trailing_newline: false,
            };
        }
        let trailing_newline = content.ends_with('\n');
        let mut lines: Vec<String> = content
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
            .collect();
        if trailing_newline {
            lines.pop();
        }
        Self {
            lines,
            eol,
            trailing_newline,
        }
    }
}

enum Region {
    Absent,
    Present { begin: usize, end: usize },
}

fn locate(lines: &[String]) -> Result<Region, FenceError> {
    let begins: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_begin_line(l))
        .map(|(i, _)| i)
        .collect();
    let ends: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| is_end_line(l))
        .map(|(i, _)| i)
        .collect();

    match (begins.len(), ends.len()) {
        (0, 0) => Ok(Region::Absent),
        (1, 1) => {
            if begins[0] < ends[0] {
                Ok(Region::Present {
                    begin: begins[0],
                    end: ends[0],
                })
            } else {
                Err(FenceError::CrossedFence)
            }
        }
        (1, 0) => Err(FenceError::UnclosedBegin),
        (0, 1) => Err(FenceError::UnopenedEnd),
        (b, _) if b > 1 => Err(FenceError::DuplicateBegin(b)),
        (_, e) => Err(FenceError::DuplicateEnd(e)),
    }
}

/// The result of splicing a rendered block into a file's content.
pub(crate) struct SpliceResult {
    pub content: String,
    pub was_present: bool,
}

/// Insert or replace the managed block, preserving every surrounding line and
/// the file's own line ending. A block is appended after a blank separator when
/// absent, or spliced over the existing region when present.
pub(crate) fn splice(
    content: &str,
    block: &str,
) -> Result<SpliceResult, FenceError> {
    let doc = Document::parse(content);
    let region = locate(&doc.lines)?;
    let block_lines: Vec<&str> = block.split('\n').collect();
    let eol = doc.eol.as_str();

    match region {
        Region::Absent => {
            let mut out: Vec<&str> =
                doc.lines.iter().map(String::as_str).collect();
            if !doc.lines.is_empty() {
                out.push("");
            }
            out.extend(block_lines.iter().copied());
            let mut rendered = out.join(eol);
            rendered.push_str(eol);
            Ok(SpliceResult {
                content: rendered,
                was_present: false,
            })
        }
        Region::Present { begin, end } => {
            let mut out: Vec<&str> = Vec::new();
            out.extend(doc.lines[..begin].iter().map(String::as_str));
            out.extend(block_lines.iter().copied());
            out.extend(doc.lines[end + 1..].iter().map(String::as_str));
            let mut rendered = out.join(eol);
            if doc.trailing_newline {
                rendered.push_str(eol);
            }
            Ok(SpliceResult {
                content: rendered,
                was_present: true,
            })
        }
    }
}

/// Remove the managed block, preserving every surrounding line. Returns `None`
/// when the file carries no block.
pub(crate) fn remove(content: &str) -> Result<Option<String>, FenceError> {
    let doc = Document::parse(content);
    match locate(&doc.lines)? {
        Region::Absent => Ok(None),
        Region::Present { begin, end } => {
            let mut out: Vec<&str> = Vec::new();
            out.extend(doc.lines[..begin].iter().map(String::as_str));
            out.extend(doc.lines[end + 1..].iter().map(String::as_str));
            let mut rendered = out.join(doc.eol.as_str());
            if doc.trailing_newline && !out.is_empty() {
                rendered.push_str(doc.eol.as_str());
            }
            Ok(Some(rendered))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "# @@omni-managed:begin (managed by `omni ignore sync`; do not edit by hand)\n/a\n/b\n# @@omni-managed:end deadbeef";

    #[test]
    fn absent_block_is_appended_after_a_blank_separator() {
        let out = splice("target\ndist\n", BLOCK).unwrap();
        assert!(!out.was_present);
        assert_eq!(out.content, format!("target\ndist\n\n{BLOCK}\n"));
    }

    #[test]
    fn absent_block_in_empty_file_has_no_leading_blank() {
        let out = splice("", BLOCK).unwrap();
        assert!(!out.was_present);
        assert_eq!(out.content, format!("{BLOCK}\n"));
    }

    #[test]
    fn present_block_is_spliced_and_surrounding_lines_preserved() {
        let seed = format!("head\n{BLOCK}\ntail\n");
        let updated = "# @@omni-managed:begin (managed by `omni ignore sync`; do not edit by hand)\n/c\n# @@omni-managed:end feedface";
        let out = splice(&seed, updated).unwrap();
        assert!(out.was_present);
        assert_eq!(out.content, format!("head\n{updated}\ntail\n"));
    }

    #[test]
    fn missing_trailing_newline_is_preserved_on_splice() {
        let seed = format!("head\n{BLOCK}");
        let out = splice(&seed, BLOCK).unwrap();
        assert_eq!(out.content, format!("head\n{BLOCK}"));
    }

    #[test]
    fn crlf_files_keep_crlf() {
        let seed = "target\r\ndist\r\n";
        let out = splice(seed, BLOCK).unwrap();
        let expected_block = BLOCK.replace('\n', "\r\n");
        assert_eq!(
            out.content,
            format!("target\r\ndist\r\n\r\n{expected_block}\r\n")
        );
    }

    #[test]
    fn duplicate_begin_is_malformed() {
        let seed = format!("{BLOCK}\n{BLOCK}\n");
        assert!(matches!(
            splice(&seed, BLOCK),
            Err(FenceError::DuplicateBegin(2))
        ));
    }

    #[test]
    fn lone_begin_is_malformed() {
        let seed = "# @@omni-managed:begin (x)\n/a\n";
        assert!(matches!(
            splice(seed, BLOCK),
            Err(FenceError::UnclosedBegin)
        ));
    }

    #[test]
    fn crossed_fence_is_malformed() {
        let seed =
            "# @@omni-managed:end deadbeef\n/a\n# @@omni-managed:begin (x)\n";
        assert!(matches!(splice(seed, BLOCK), Err(FenceError::CrossedFence)));
    }

    #[test]
    fn remove_deletes_only_the_block() {
        let seed = format!("head\n{BLOCK}\ntail\n");
        let out = remove(&seed).unwrap().unwrap();
        assert_eq!(out, "head\ntail\n");
    }

    #[test]
    fn remove_reports_absent_when_no_block() {
        assert_eq!(remove("target\n").unwrap(), None);
    }
}
