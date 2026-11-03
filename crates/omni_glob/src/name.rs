//! A small glob matcher for *names* (environment variables, dotted argument
//! paths, ...), as opposed to file paths.
//!
//! Syntax: `*` (any run of characters within a segment), `?` (one character),
//! `**` (a whole segment: zero or more segments, only meaningful with a
//! separator) and literals. `\*`, `\?` and `\\` escape the metacharacters.
//! There are no character classes, no negation and no regex.
//!
//! Callers hold an opaque [`NameGlobSet`], so the engine can change without
//! touching the API.

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Separator {
    /// Names are flat: `*` matches the whole name.
    #[default]
    None,
    /// Names are split on this character; `*` stays within a segment.
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Case {
    #[default]
    Sensitive,
    Insensitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct NameGlobOptions {
    pub separator: Separator,
    pub case: Case,
}

impl NameGlobOptions {
    pub fn separator(mut self, s: Separator) -> Self {
        self.separator = s;
        self
    }

    pub fn case(mut self, c: Case) -> Self {
        self.case = c;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid name glob `{pattern}`: {kind}")]
pub struct NameGlobError {
    pattern: String,
    kind: NameGlobErrorKind,
}

impl NameGlobError {
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn kind(&self) -> &NameGlobErrorKind {
        &self.kind
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum NameGlobErrorKind {
    #[error("pattern is empty")]
    Empty,
    #[error("pattern ends with a dangling `\\`")]
    DanglingEscape,
    #[error("`{0}` cannot be used as a separator")]
    InvalidSeparator(char),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Lit(char),
    One,
    Any,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    Globstar,
    Toks(Vec<Tok>),
}

#[derive(Debug, Clone)]
struct Compiled {
    source: String,
    segs: Vec<Seg>,
}

#[derive(Debug, Clone)]
pub struct NameGlobSet {
    options: NameGlobOptions,
    patterns: Vec<Compiled>,
    /// Wildcard-free patterns, normalized for case, for O(1) lookup.
    exact: HashSet<String>,
    wild: Vec<usize>,
}

impl NameGlobSet {
    pub fn new<I, S>(
        patterns: I,
        options: &NameGlobOptions,
    ) -> Result<Self, NameGlobError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        if let Separator::Char(c) = options.separator
            && matches!(c, '*' | '?' | '\\')
        {
            return Err(NameGlobError {
                pattern: String::new(),
                kind: NameGlobErrorKind::InvalidSeparator(c),
            });
        }

        let mut compiled = Vec::new();
        let mut exact = HashSet::new();
        let mut wild = Vec::new();
        for p in patterns {
            let p = p.as_ref();
            let (segs, literal) = compile(p, options)?;
            let idx = compiled.len();
            match literal {
                Some(lit) => {
                    exact.insert(lit);
                }
                None => wild.push(idx),
            }
            compiled.push(Compiled {
                source: p.to_string(),
                segs,
            });
        }
        Ok(Self {
            options: *options,
            patterns: compiled,
            exact,
            wild,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn is_match(&self, name: &str) -> bool {
        let name = self.fold(name);
        if self.exact.contains(name.as_str()) {
            return true;
        }
        let parts = self.split(&name);
        self.wild
            .iter()
            .any(|&i| match_segs(&self.patterns[i].segs, &parts))
    }

    /// Patterns that matched none of `names`; for debug-level typo hints.
    pub fn unmatched<'a>(
        &'a self,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Vec<&'a str> {
        let names: Vec<String> =
            names.into_iter().map(|n| self.fold(n)).collect();
        self.patterns
            .iter()
            .filter(|p| {
                !names.iter().any(|n| match_segs(&p.segs, &self.split(n)))
            })
            .map(|p| p.source.as_str())
            .collect()
    }

    fn fold(&self, s: &str) -> String {
        match self.options.case {
            Case::Sensitive => s.to_string(),
            Case::Insensitive => s.to_lowercase(),
        }
    }

    fn split<'n>(&self, name: &'n str) -> Vec<&'n str> {
        match self.options.separator {
            Separator::None => vec![name],
            Separator::Char(c) => name.split(c).collect(),
        }
    }
}

/// Returns the segments and, for wildcard-free patterns, the folded literal.
fn compile(
    pattern: &str,
    options: &NameGlobOptions,
) -> Result<(Vec<Seg>, Option<String>), NameGlobError> {
    let err = |kind| NameGlobError {
        pattern: pattern.to_string(),
        kind,
    };
    if pattern.is_empty() {
        return Err(err(NameGlobErrorKind::Empty));
    }
    let folded = match options.case {
        Case::Sensitive => pattern.to_string(),
        Case::Insensitive => pattern.to_lowercase(),
    };

    // Tokenize, tracking separators as their own marker (None) so escaped
    // separators are impossible and splitting stays simple.
    let mut segs: Vec<Seg> = Vec::new();
    let mut cur: Vec<Tok> = Vec::new();
    let mut raw_star_only = true;
    let mut wildcard = false;
    let mut chars = folded.chars();
    let flush =
        |cur: &mut Vec<Tok>, raw_star_only: &mut bool, segs: &mut Vec<Seg>| {
            if sep_is_char(options)
                && *raw_star_only
                && *cur == [Tok::Any, Tok::Any]
            {
                segs.push(Seg::Globstar);
                cur.clear();
            } else {
                segs.push(Seg::Toks(std::mem::take(cur)));
            }
            *raw_star_only = true;
        };
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(n) => {
                    raw_star_only = false;
                    cur.push(Tok::Lit(n));
                }
                None => return Err(err(NameGlobErrorKind::DanglingEscape)),
            },
            '*' => {
                wildcard = true;
                // Collapse runs, but remember `**` for globstar detection.
                cur.push(Tok::Any);
            }
            '?' => {
                wildcard = true;
                raw_star_only = false;
                cur.push(Tok::One);
            }
            c if Separator::Char(c) == options.separator => {
                flush(&mut cur, &mut raw_star_only, &mut segs);
            }
            c => {
                raw_star_only = false;
                cur.push(Tok::Lit(c));
            }
        }
    }
    flush(&mut cur, &mut raw_star_only, &mut segs);

    // Collapse consecutive `*` inside non-globstar segments.
    for s in &mut segs {
        if let Seg::Toks(t) = s {
            t.dedup_by(|a, b| *a == Tok::Any && *b == Tok::Any);
        }
    }

    let literal = if wildcard {
        None
    } else {
        // Re-derive the literal with escapes resolved.
        let mut out = String::new();
        let (first, rest) = segs.split_first().expect("at least one segment");
        let push = |out: &mut String, s: &Seg| {
            if let Seg::Toks(t) = s {
                for tok in t {
                    if let Tok::Lit(c) = tok {
                        out.push(*c);
                    }
                }
            }
        };
        push(&mut out, first);
        for s in rest {
            if let Separator::Char(c) = options.separator {
                out.push(c);
            }
            push(&mut out, s);
        }
        Some(out)
    };
    Ok((segs, literal))
}

fn sep_is_char(o: &NameGlobOptions) -> bool {
    matches!(o.separator, Separator::Char(_))
}

fn match_segs(pat: &[Seg], name: &[&str]) -> bool {
    match pat.split_first() {
        None => name.is_empty(),
        Some((Seg::Globstar, rest)) => {
            (0..=name.len()).any(|skip| match_segs(rest, &name[skip..]))
        }
        Some((Seg::Toks(t), rest)) => match name.split_first() {
            Some((n, tail)) => match_toks(t, n) && match_segs(rest, tail),
            None => false,
        },
    }
}

fn match_toks(toks: &[Tok], name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match toks.get(p) {
            Some(Tok::Any) => {
                star = Some((p, n));
                p += 1;
            }
            Some(Tok::One) => {
                p += 1;
                n += 1;
            }
            Some(Tok::Lit(c)) if *c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    p = sp + 1;
                    n = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    toks[p..].iter().all(|t| *t == Tok::Any)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> NameGlobOptions {
        NameGlobOptions::default()
    }

    fn dotted() -> NameGlobOptions {
        NameGlobOptions::default().separator(Separator::Char('.'))
    }

    fn m(pat: &str, opts: &NameGlobOptions, name: &str) -> bool {
        NameGlobSet::new([pat], opts).unwrap().is_match(name)
    }

    #[test]
    fn case_table() {
        let cases: &[(&str, NameGlobOptions, &str, bool)] = &[
            ("TOKEN", flat(), "TOKEN", true),
            ("TOKEN", flat(), "token", false),
            ("*_TOKEN", flat(), "GITHUB_TOKEN", true),
            ("*_TOKEN", flat(), "TOKEN", false),
            ("AWS_*", flat(), "AWS_", true),
            ("AWS_*", flat(), "AWS_KEY", true),
            ("A?C", flat(), "ABC", true),
            ("A?C", flat(), "AC", false),
            ("*", flat(), "", true),
            ("*", flat(), "a.b", true),
            ("a.*", dotted(), "a.b", true),
            ("a.*", dotted(), "a.b.c", false),
            ("a.**", dotted(), "a.b.c", true),
            ("a.**", dotted(), "a", true),
            ("**.token", dotted(), "x.y.token", true),
            ("**.token", dotted(), "token", true),
            ("a.b", dotted(), "a.b", true),
            ("a.b", dotted(), "a.c", false),
            ("a\\*b", flat(), "a*b", true),
            ("a\\*b", flat(), "aXb", false),
            ("a\\\\b", flat(), "a\\b", true),
            ("a\\?", flat(), "a?", true),
            ("a\\?", flat(), "ab", false),
            ("a*b*c", flat(), "aXXbYYc", true),
            ("a*b*c", flat(), "aXXbYY", false),
            ("a**b", flat(), "aXb", true),
        ];
        for (p, o, n, want) in cases {
            assert_eq!(m(p, o, n), *want, "pattern {p:?} name {n:?}");
        }
    }

    #[test]
    fn case_insensitive() {
        let o = flat().case(Case::Insensitive);
        assert!(m("path", &o, "PATH"));
        assert!(m("PA*", &o, "path"));
    }

    #[test]
    fn invalid_patterns() {
        assert_eq!(
            NameGlobSet::new([""], &flat()).unwrap_err().kind(),
            &NameGlobErrorKind::Empty
        );
        assert_eq!(
            NameGlobSet::new(["a\\"], &flat()).unwrap_err().kind(),
            &NameGlobErrorKind::DanglingEscape
        );
        assert!(
            NameGlobSet::new(["a"], &flat().separator(Separator::Char('*')))
                .is_err()
        );
    }

    #[test]
    fn empty_name_matches_only_star() {
        assert!(!m("a", &flat(), ""));
        assert!(m("*", &flat(), ""));
    }

    #[test]
    fn unmatched_reports_typos() {
        let s = NameGlobSet::new(["pasword", "tok*"], &flat()).unwrap();
        assert_eq!(s.unmatched(["password", "token"]), vec!["pasword"]);
    }

    #[test]
    fn multi_pattern_set() {
        let s = NameGlobSet::new(["A", "B_*"], &flat()).unwrap();
        assert!(s.is_match("A"));
        assert!(s.is_match("B_x"));
        assert!(!s.is_match("C"));
    }
}
