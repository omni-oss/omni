//! Escape-aware task references (RFC 0024 F03.2, RFC §7.4).
//!
//! One full-input parser and one canonical formatter for the text form of a task
//! reference, so no consumer splits on `#` or `::` itself.
//!
//! # Grammar
//!
//! ```text
//! reference := ["^"] [project "#"] [plugin "::"] task
//! ```
//!
//! Only unescaped separators carry syntax: `#` ends the project, `::` ends the
//! plugin (activation scope), and a leading `^` selects upstream lookup. A single
//! `:` is an ordinary character, so `lint:fix` is one task name. A backslash
//! escapes the next character, which must be one of `\`, `#`, `:` or `^`:
//! `\#` is a literal hash, `\:` a literal colon, `\\` a literal backslash and
//! `\^` a literal caret. Any other escape, a trailing backslash, an empty
//! component or an extra separator is an error; nothing is matched as a
//! substring and no suffix is dropped.
//!
//! So `native::build` is task `build` of plugin `native`, while `native\:\:build`
//! is the unscoped task literally named `native::build`; `build::debug`,
//! `build\:\:debug` and `build#debug` are three different identities.
//!
//! Escaping is **injective**: formatting a reference and parsing the result gives
//! the original components, for every possible component text. The formatter
//! escapes `\` and `#` everywhere, every colon that touches another colon, a
//! plugin's trailing colon (which would otherwise merge into the `::` that
//! follows it), and a `^` that starts the output.
//!
//! # Raw components
//!
//! Components given as structured data (a handwritten `{ project, task }` object,
//! or plugin output) are raw: [`TaskReference::from_raw_components`] validates but
//! never unescapes or reparses them. An object `{ project: common, task:
//! "native::build" }` is the task literally named `native::build`; it does not
//! become plugin `native`.
//!
//! # Not every reference is valid everywhere
//!
//! Parsing is shared; what a surface accepts is not. After parsing once, call
//! [`TaskReference::validate_for`] with the surface (declaration keys, `extends`
//! and alias targets, dependencies and `with`, exact selectors).
//!
//! Not in this module: root references (a `ProjectRootId` lowered to a project
//! name after composition) and the legacy-`::` declaration migration, which need
//! the composed project catalog and an activation registry.

use std::fmt;

/// A parsed task reference with raw (decoded) components. Fields are private so
/// every value comes from a validating constructor and always formats to text
/// that parses back to it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskReference {
    /// A leading unescaped `^`: look the task up in upstream projects.
    upstream: bool,
    /// The explicit project, before `#`.
    project: Option<String>,
    /// The plugin (activation) the task is scoped to, before `::`.
    plugin: Option<String>,
    /// The raw local task name.
    task: String,
}

/// A part of a reference, named in errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceComponent {
    Project,
    Plugin,
    Task,
}

impl fmt::Display for ReferenceComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Project => "project",
            Self::Plugin => "plugin",
            Self::Task => "task",
        })
    }
}

/// An unescaped separator, named in errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceSeparator {
    /// `#`, between the project and the rest.
    Project,
    /// `::`, between the plugin and the task.
    Plugin,
}

impl fmt::Display for ReferenceSeparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Project => "`#`",
            Self::Plugin => "`::`",
        })
    }
}

/// Where a reference is used. Each surface accepts a subset of reference kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceSurface {
    /// A handwritten task declaration key.
    DeclarationKey,
    /// A task's `extends` entry or an alias target.
    ExtendsOrAliasTarget,
    /// A task's `dependencies` or `with` entry.
    DependencyOrWith,
    /// An exact CLI or MCP task selector.
    ExactSelector,
}

impl fmt::Display for ReferenceSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::DeclarationKey => "a task declaration key",
            Self::ExtendsOrAliasTarget => "an `extends` entry or alias target",
            Self::DependencyOrWith => "a `dependencies` or `with` entry",
            Self::ExactSelector => "an exact task selector",
        })
    }
}

/// Why a reference could not be parsed or is not valid on a surface. Positions
/// are byte offsets into the input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReferenceError {
    #[error(
        "unsupported escape `\\{found}` at byte {index}; only `\\\\`, `\\#`, `\\:` and `\\^` are escapes"
    )]
    UnsupportedEscape { found: char, index: usize },

    #[error("the reference ends with a dangling backslash at byte {index}")]
    DanglingBackslash { index: usize },

    #[error("the {component} is empty{}", at(*index))]
    Empty {
        component: ReferenceComponent,
        /// Byte offset where the component ends. `None` for raw components.
        index: Option<usize>,
    },

    #[error(
        "extra unescaped {separator} at byte {index}; escape a literal one with a backslash"
    )]
    ExtraSeparator {
        separator: ReferenceSeparator,
        index: usize,
    },

    #[error(
        "the project cannot be plugin-scoped: unescaped `::` at byte {index} comes before `#`; escape a literal `::` in a project name"
    )]
    ScopedProject { index: usize },

    #[error("the {component} contains a NUL character{}", at(*index))]
    Nul {
        component: ReferenceComponent,
        /// Byte offset of the NUL. `None` for raw components.
        index: Option<usize>,
    },

    #[error(
        "an upstream reference (`^`) cannot name a project; use `project#task` or `^task`"
    )]
    UpstreamWithProject,

    #[error("{surface} cannot {reason}")]
    NotAllowed {
        surface: ReferenceSurface,
        reason: &'static str,
    },
}

fn at(index: Option<usize>) -> String {
    index.map(|i| format!(" at byte {i}")).unwrap_or_default()
}

const ESCAPABLE: [char; 4] = ['\\', '#', ':', '^'];

impl TaskReference {
    /// A reference with only a task name. Fails on an empty name or a NUL.
    pub fn own(task: impl Into<String>) -> Result<Self, ReferenceError> {
        Self::from_raw_components(false, None, None, task.into())
    }

    /// Whether this is an upstream (`^`) reference.
    pub fn upstream(&self) -> bool {
        self.upstream
    }

    /// The explicit project, if any.
    pub fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }

    /// The plugin the task is scoped to, if any.
    pub fn plugin(&self) -> Option<&str> {
        self.plugin.as_deref()
    }

    /// The raw local task name.
    pub fn task(&self) -> &str {
        &self.task
    }

    /// Build a reference from raw components, for structured input. Nothing is
    /// unescaped or reparsed: `task` is exactly the local name, whatever it
    /// contains. Fails on an empty component or a NUL.
    pub fn from_raw_components(
        upstream: bool,
        project: Option<String>,
        plugin: Option<String>,
        task: String,
    ) -> Result<Self, ReferenceError> {
        let reference = Self {
            upstream,
            project,
            plugin,
            task,
        };
        reference.check_components()?;
        if reference.upstream && reference.project.is_some() {
            return Err(ReferenceError::UpstreamWithProject);
        }
        Ok(reference)
    }

    fn check_components(&self) -> Result<(), ReferenceError> {
        let parts = [
            (ReferenceComponent::Project, self.project.as_deref()),
            (ReferenceComponent::Plugin, self.plugin.as_deref()),
            (ReferenceComponent::Task, Some(self.task.as_str())),
        ];
        for (component, value) in parts {
            let Some(value) = value else { continue };
            if value.is_empty() {
                return Err(ReferenceError::Empty {
                    component,
                    index: None,
                });
            }
            if value.contains('\0') {
                return Err(ReferenceError::Nul {
                    component,
                    index: None,
                });
            }
        }
        Ok(())
    }

    /// Parse the whole input as one reference.
    pub fn parse(input: &str) -> Result<Self, ReferenceError> {
        let mut chars = input.char_indices().peekable();
        let mut upstream = false;
        if let Some(&(_, '^')) = chars.peek() {
            chars.next();
            upstream = true;
        }

        let mut project: Option<String> = None;
        let mut plugin: Option<String> = None;
        let mut current = String::new();
        let mut nul_at: Option<usize> = None;

        while let Some((index, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    None => {
                        return Err(ReferenceError::DanglingBackslash {
                            index,
                        });
                    }
                    Some((_, escaped)) if ESCAPABLE.contains(&escaped) => {
                        current.push(escaped);
                    }
                    Some((_, found)) => {
                        return Err(ReferenceError::UnsupportedEscape {
                            found,
                            index,
                        });
                    }
                },
                '#' => {
                    if project.is_some() {
                        return Err(ReferenceError::ExtraSeparator {
                            separator: ReferenceSeparator::Project,
                            index,
                        });
                    }
                    project = Some(take_component(
                        &mut current,
                        &mut nul_at,
                        index,
                        ReferenceComponent::Project,
                    )?);
                }
                ':' if matches!(chars.peek(), Some(&(_, ':'))) => {
                    chars.next();
                    if plugin.is_some() {
                        return Err(ReferenceError::ExtraSeparator {
                            separator: ReferenceSeparator::Plugin,
                            index,
                        });
                    }
                    // A `::` before the `#` would scope the project itself.
                    if project.is_none() && has_unescaped_hash(&input[index..])
                    {
                        return Err(ReferenceError::ScopedProject { index });
                    }
                    plugin = Some(take_component(
                        &mut current,
                        &mut nul_at,
                        index,
                        ReferenceComponent::Plugin,
                    )?);
                }
                other => {
                    if other == '\0' {
                        nul_at.get_or_insert(index);
                    }
                    current.push(other);
                }
            }
        }

        let task = take_component(
            &mut current,
            &mut nul_at,
            input.len(),
            ReferenceComponent::Task,
        )?;
        let reference = Self {
            upstream,
            project,
            plugin,
            task,
        };
        reference.check_components()?;
        Ok(reference)
    }

    /// The canonical text form. [`parse`](Self::parse) of it returns this
    /// reference for any component text.
    pub fn format(&self) -> String {
        let mut out = String::new();
        if self.upstream {
            out.push('^');
        }
        let mut first = true;
        if let Some(project) = &self.project {
            escape_into(&mut out, project, &mut first, false);
            out.push('#');
        }
        if let Some(plugin) = &self.plugin {
            escape_into(&mut out, plugin, &mut first, true);
            out.push_str("::");
        }
        escape_into(&mut out, &self.task, &mut first, false);
        out
    }

    /// Check that this kind of reference is allowed where it is used. The
    /// parser accepts every kind; surfaces restrict them (RFC §7.4).
    pub fn validate_for(
        &self,
        surface: ReferenceSurface,
    ) -> Result<(), ReferenceError> {
        let deny = |reason| ReferenceError::NotAllowed { surface, reason };
        match surface {
            ReferenceSurface::DeclarationKey
            | ReferenceSurface::ExtendsOrAliasTarget => {
                if self.upstream {
                    return Err(deny("be an upstream reference (`^`)"));
                }
                if self.project.is_some() {
                    return Err(deny(
                        "name another project; it is same-project only",
                    ));
                }
            }
            ReferenceSurface::DependencyOrWith => {
                if self.upstream && self.project.is_some() {
                    return Err(ReferenceError::UpstreamWithProject);
                }
            }
            ReferenceSurface::ExactSelector => {
                if self.upstream {
                    return Err(deny(
                        "be an upstream reference (`^`); write `\\^name` to select a task literally named `^name`",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl fmt::Display for TaskReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

impl std::str::FromStr for TaskReference {
    type Err = ReferenceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

fn take_component(
    current: &mut String,
    nul_at: &mut Option<usize>,
    end: usize,
    component: ReferenceComponent,
) -> Result<String, ReferenceError> {
    if current.is_empty() {
        return Err(ReferenceError::Empty {
            component,
            index: Some(end),
        });
    }
    if let Some(index) = nul_at.take() {
        return Err(ReferenceError::Nul {
            component,
            index: Some(index),
        });
    }
    Ok(std::mem::take(current))
}

/// Whether `s` has a `#` that is not escaped, scanning escapes the same way the
/// parser does.
fn has_unescaped_hash(s: &str) -> bool {
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '#' => return true,
            _ => {}
        }
    }
    false
}

/// Escape one component into `out`. `first` tracks whether nothing has been
/// written yet, because only a `^` at the very start of the output would be read
/// as the upstream marker.
fn escape_into(
    out: &mut String,
    component: &str,
    first: &mut bool,
    is_plugin: bool,
) {
    let chars: Vec<char> = component.chars().collect();
    let last = chars.len().saturating_sub(1);
    for (i, &c) in chars.iter().enumerate() {
        let touches_colon =
            (i > 0 && chars[i - 1] == ':') || (i < last && chars[i + 1] == ':');
        // A plugin's trailing colon would merge with the `::` that follows it.
        let trailing_plugin_colon = is_plugin && i == last;
        let escape = match c {
            '\\' | '#' => true,
            ':' => touches_colon || trailing_plugin_colon,
            '^' => *first && i == 0,
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    *first = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> TaskReference {
        TaskReference::parse(s).unwrap_or_else(|e| panic!("{s:?}: {e}"))
    }

    fn reference(
        upstream: bool,
        project: Option<&str>,
        plugin: Option<&str>,
        task: &str,
    ) -> TaskReference {
        TaskReference {
            upstream,
            project: project.map(str::to_string),
            plugin: plugin.map(str::to_string),
            task: task.to_string(),
        }
    }

    #[test]
    fn a_single_colon_is_an_ordinary_name() {
        assert_eq!(parse("lint:fix"), TaskReference::own("lint:fix").unwrap());
        assert_eq!(parse("a:b:c"), TaskReference::own("a:b:c").unwrap());
    }

    #[test]
    fn scoped_and_literal_double_colons_are_different_identities() {
        let scoped = parse("native::build");
        assert_eq!(scoped, reference(false, None, Some("native"), "build"));
        let literal = parse(r"native\:\:build");
        assert_eq!(literal, TaskReference::own("native::build").unwrap());
        assert_ne!(scoped, literal);
    }

    #[test]
    fn the_rfc_examples_parse_as_documented() {
        assert_eq!(
            parse(r"app#native::check\#fast"),
            reference(false, Some("app"), Some("native"), "check#fast")
        );
        // The three names the RFC says must stay distinct.
        let a = parse("build::debug");
        let b = parse(r"build\:\:debug");
        let c = parse("build#debug");
        assert_eq!(a, reference(false, None, Some("build"), "debug"));
        assert_eq!(b, TaskReference::own("build::debug").unwrap());
        assert_eq!(c, reference(false, Some("build"), None, "debug"));
        assert!(a != b && b != c && a != c);
    }

    #[test]
    fn upstream_and_explicit_project_forms_parse() {
        assert_eq!(parse("^build"), reference(true, None, None, "build"));
        assert_eq!(
            parse("app#build"),
            reference(false, Some("app"), None, "build")
        );
        assert_eq!(parse(r"\^build"), TaskReference::own("^build").unwrap());
        // Only a leading caret is syntax.
        assert_eq!(parse("a^b"), TaskReference::own("a^b").unwrap());
        assert_eq!(parse("app#^build").task, "^build");
        assert_eq!(parse("^^x"), reference(true, None, None, "^x"));
    }

    #[test]
    fn escapes_decode_once() {
        assert_eq!(parse(r"a\\b").task, r"a\b");
        assert_eq!(parse(r"a\#b").task, "a#b");
        assert_eq!(parse(r"a\:b").task, "a:b");
        assert_eq!(parse(r"\\\#").task, r"\#");
    }

    #[test]
    fn malformed_input_is_an_error_not_a_substring_match() {
        use ReferenceComponent::*;
        let err = |s: &str| TaskReference::parse(s).unwrap_err();

        assert_eq!(
            err(r"a\n"),
            ReferenceError::UnsupportedEscape {
                found: 'n',
                index: 1
            }
        );
        assert_eq!(
            err(r"a\ "),
            ReferenceError::UnsupportedEscape {
                found: ' ',
                index: 1
            }
        );
        assert_eq!(err("a\\"), ReferenceError::DanglingBackslash { index: 1 });
        assert_eq!(
            err(""),
            ReferenceError::Empty {
                component: Task,
                index: Some(0)
            }
        );
        assert_eq!(
            err("^"),
            ReferenceError::Empty {
                component: Task,
                index: Some(1)
            }
        );
        assert_eq!(
            err("#x"),
            ReferenceError::Empty {
                component: Project,
                index: Some(0)
            }
        );
        assert_eq!(
            err("x#"),
            ReferenceError::Empty {
                component: ReferenceComponent::Task,
                index: Some(2)
            }
        );
        assert_eq!(
            err("::x"),
            ReferenceError::Empty {
                component: Plugin,
                index: Some(0)
            }
        );
        assert_eq!(
            err("x::"),
            ReferenceError::Empty {
                component: Task,
                index: Some(3)
            }
        );
        assert_eq!(
            err("a#b#c"),
            ReferenceError::ExtraSeparator {
                separator: ReferenceSeparator::Project,
                index: 3
            }
        );
        assert_eq!(
            err("a::b::c"),
            ReferenceError::ExtraSeparator {
                separator: ReferenceSeparator::Plugin,
                index: 4
            }
        );
        assert_eq!(err("a::b#c"), ReferenceError::ScopedProject { index: 1 });
    }

    #[test]
    fn a_double_colon_before_the_hash_is_a_scoped_project() {
        assert_eq!(
            TaskReference::parse("a::b#c").unwrap_err(),
            ReferenceError::ScopedProject { index: 1 }
        );
        // The same text with a literal `::` in the project is fine.
        assert_eq!(
            parse(r"a\:\:b#c"),
            reference(false, Some("a::b"), None, "c")
        );
    }

    #[test]
    fn errors_name_what_is_wrong() {
        let message =
            |s: &str| TaskReference::parse(s).unwrap_err().to_string();
        assert!(message(r"a\n").contains(r"\n"));
        assert!(message("a\\").contains("dangling"));
        assert!(message("a#b#c").contains("extra unescaped `#`"));
        assert!(message("").contains("task is empty"));
    }

    #[test]
    fn a_triple_colon_is_one_separator_then_a_literal_colon() {
        assert_eq!(parse("a:::b"), reference(false, None, Some("a"), ":b"));
        assert_eq!(parse("a::\\:b").task, ":b");
    }

    #[test]
    fn object_components_are_raw_and_never_reparsed() {
        // The task is the literal local name `native::build` ...
        let literal = TaskReference::from_raw_components(
            false,
            Some("common".into()),
            None,
            "native::build".into(),
        )
        .unwrap();
        assert_eq!(literal.plugin, None);
        assert_eq!(literal.task, "native::build");
        assert_eq!(literal.format(), r"common#native\:\:build");
        // ... and naming the plugin explicitly is how it becomes scoped.
        let scoped = TaskReference::from_raw_components(
            false,
            Some("common".into()),
            Some("native".into()),
            "build".into(),
        )
        .unwrap();
        assert_eq!(scoped.format(), "common#native::build");
        assert_ne!(literal, scoped);
        // Backslashes in raw components are data, not escapes.
        let raw = TaskReference::from_raw_components(
            false,
            None,
            None,
            r"a\#b".into(),
        )
        .unwrap();
        assert_eq!(raw.task, r"a\#b");
        assert_eq!(raw.format(), r"a\\\#b");
    }

    #[test]
    fn raw_components_are_validated() {
        let raw = |p: Option<&str>, g: Option<&str>, t: &str| {
            TaskReference::from_raw_components(
                false,
                p.map(str::to_string),
                g.map(str::to_string),
                t.to_string(),
            )
        };
        assert_eq!(
            TaskReference::own("").unwrap_err(),
            ReferenceError::Empty {
                component: ReferenceComponent::Task,
                index: None
            }
        );
        assert!(TaskReference::own("a\0").is_err());
        assert_eq!(
            TaskReference::parse("é\0").unwrap_err(),
            ReferenceError::Nul {
                component: ReferenceComponent::Task,
                index: Some(2)
            }
        );
        assert_eq!(
            raw(None, None, "").unwrap_err(),
            ReferenceError::Empty {
                component: ReferenceComponent::Task,
                index: None
            }
        );
        assert_eq!(
            raw(Some(""), None, "t").unwrap_err(),
            ReferenceError::Empty {
                component: ReferenceComponent::Project,
                index: None
            }
        );
        assert_eq!(
            raw(None, Some(""), "t").unwrap_err(),
            ReferenceError::Empty {
                component: ReferenceComponent::Plugin,
                index: None
            }
        );
        assert_eq!(
            raw(None, None, "a\0b").unwrap_err(),
            ReferenceError::Nul {
                component: ReferenceComponent::Task,
                index: None
            }
        );
        assert_eq!(
            TaskReference::from_raw_components(
                true,
                Some("p".into()),
                None,
                "t".into()
            )
            .unwrap_err(),
            ReferenceError::UpstreamWithProject
        );
    }

    #[test]
    fn the_formatter_escapes_what_would_be_read_as_syntax() {
        let f = |p: Option<&str>, g: Option<&str>, t: &str, up: bool| {
            reference(up, p, g, t).format()
        };
        assert_eq!(f(None, None, "build::debug", false), r"build\:\:debug");
        assert_eq!(f(None, None, "build#debug", false), r"build\#debug");
        assert_eq!(f(None, None, "lint:fix", false), "lint:fix");
        assert_eq!(f(None, None, "^build", false), r"\^build");
        assert_eq!(f(None, None, "a^b", false), "a^b");
        assert_eq!(f(None, None, "^x", true), r"^\^x");
        assert_eq!(f(Some("^app"), None, "t", false), r"\^app#t");
        assert_eq!(
            f(None, Some("native"), "check#fast", false),
            r"native::check\#fast"
        );
        // A plugin's trailing colon must not merge with the separator.
        assert_eq!(f(None, Some("p:"), "x", false), r"p\:::x");
        assert_eq!(f(None, None, r"a\b", false), r"a\\b");
    }

    #[test]
    fn surfaces_restrict_which_references_are_valid() {
        use ReferenceSurface::*;
        let up = parse("^build");
        let project = parse("other#build");
        let scoped = parse("native::build");
        let own = parse("build");
        let literal_caret = parse(r"\^build");

        for surface in [DeclarationKey, ExtendsOrAliasTarget] {
            assert!(own.validate_for(surface).is_ok());
            assert!(scoped.validate_for(surface).is_ok(), "{surface}");
            assert!(up.validate_for(surface).is_err());
            assert!(project.validate_for(surface).is_err());
        }
        for reference in [&own, &up, &project, &scoped] {
            assert!(reference.validate_for(DependencyOrWith).is_ok());
        }
        assert!(own.validate_for(ExactSelector).is_ok());
        assert!(project.validate_for(ExactSelector).is_ok());
        assert!(scoped.validate_for(ExactSelector).is_ok());
        assert!(up.validate_for(ExactSelector).is_err());
        // A task literally named `^build` is still selectable.
        assert!(literal_caret.validate_for(ExactSelector).is_ok());
    }

    #[test]
    fn surface_errors_say_which_surface_and_why() {
        let message = |s: &str, surface| {
            parse(s).validate_for(surface).unwrap_err().to_string()
        };
        let m = message("^build", ReferenceSurface::ExactSelector);
        assert!(
            m.contains("exact task selector") && m.contains(r"\^name"),
            "{m}"
        );
        let m = message(
            "other-project#build",
            ReferenceSurface::ExtendsOrAliasTarget,
        );
        assert!(m.contains("same-project"), "{m}");
        let m = message("^build", ReferenceSurface::ExtendsOrAliasTarget);
        assert!(m.contains("upstream"), "{m}");
    }

    // ---- exhaustive injectivity ----

    /// Every string over `alphabet` of length `1..=max`.
    fn strings(alphabet: &[char], max: usize) -> Vec<String> {
        let mut all = Vec::new();
        let mut layer = vec![String::new()];
        for _ in 0..max {
            let mut next = Vec::new();
            for prefix in &layer {
                for &c in alphabet {
                    let mut s = prefix.clone();
                    s.push(c);
                    next.push(s);
                }
            }
            all.extend(next.iter().cloned());
            layer = next;
        }
        all
    }

    const ALPHABET: [char; 5] = ['a', ':', '#', '\\', '^'];

    #[test]
    fn formatting_then_parsing_returns_the_same_components() {
        // Every project, plugin and task of up to two characters over the
        // characters that matter, in every combination of presence and `^`.
        let parts = strings(&ALPHABET, 2);
        let projects: Vec<Option<&str>> = std::iter::once(None)
            .chain(parts.iter().map(|s| Some(s.as_str())))
            .collect();
        let mut checked = 0u64;
        for upstream in [false, true] {
            for project in &projects {
                if upstream && project.is_some() {
                    continue;
                }
                for plugin in &projects {
                    for task in &parts {
                        let r = reference(upstream, *project, *plugin, task);
                        let text = r.format();
                        let back = TaskReference::parse(&text).unwrap_or_else(|e| {
                            panic!("{r:?} formatted as {text:?} did not parse: {e}")
                        });
                        assert_eq!(back, r, "{text:?}");
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 29_760, "the combination count changed");
    }

    #[test]
    fn longer_tasks_and_plugins_round_trip_next_to_each_other() {
        // Colon runs and escapes interact across the `::` separator, so also try
        // longer components on either side of it.
        let parts = strings(&ALPHABET, 4);
        for plugin in strings(&[':', 'a'], 3) {
            for task in &parts {
                for project in [None, Some("q")] {
                    let r = reference(false, project, Some(&plugin), task);
                    let text = r.format();
                    assert_eq!(
                        TaskReference::parse(&text).unwrap(),
                        r,
                        "{text:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_parseable_text_reformats_to_one_stable_canonical_form() {
        // The reverse direction: whatever parses, its canonical text parses back
        // to the same reference and is itself a fixed point of format.
        let mut parsed = 0u64;
        for text in strings(&ALPHABET, 6) {
            let Ok(r) = TaskReference::parse(&text) else {
                continue;
            };
            parsed += 1;
            let canonical = r.format();
            let again = TaskReference::parse(&canonical)
                .unwrap_or_else(|e| panic!("{text:?} -> {canonical:?}: {e}"));
            assert_eq!(again, r, "{text:?} -> {canonical:?}");
            assert_eq!(
                again.format(),
                canonical,
                "not a fixed point: {text:?}"
            );
        }
        assert!(parsed > 1_000, "only {parsed} parsed");
    }

    #[test]
    fn distinct_references_never_share_text() {
        // Injectivity as a collision check over a small space.
        use std::collections::HashMap;
        let parts = strings(&ALPHABET, 2);
        let mut seen: HashMap<String, TaskReference> = HashMap::new();
        for project in std::iter::once(None).chain(parts.iter().map(Some)) {
            for plugin in std::iter::once(None).chain(parts.iter().map(Some)) {
                for task in &parts {
                    let r = reference(
                        false,
                        project.map(String::as_str),
                        plugin.map(String::as_str),
                        task,
                    );
                    if let Some(previous) = seen.insert(r.format(), r.clone()) {
                        assert_eq!(
                            previous,
                            r,
                            "two references share {:?}",
                            r.format()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn display_and_from_str_agree_with_format_and_parse() {
        let r = parse(r"app#native::check\#fast");
        assert_eq!(r.to_string(), r.format());
        assert_eq!(r.to_string().parse::<TaskReference>().unwrap(), r);
    }
}
