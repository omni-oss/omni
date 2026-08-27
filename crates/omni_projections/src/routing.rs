use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};
use omni_projection_configurations::{
    DestPath, OmniPath, Projection, ProjectionStrategy,
};
use path_clean::PathClean;

use crate::error::{ProjectionError, Result};
use crate::guardrails::{Guardrails, check_dest};

/// One resolved link the applier will materialize: `dest_abs` points at
/// `source_abs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkPair {
    pub source_abs: PathBuf,
    pub dest_abs: PathBuf,
}

/// A source entry discovered under a source root, expressed relative to that
/// root. Populated either by a real filesystem scan or, in tests, by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedEntry {
    pub rel: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
}

impl ScannedEntry {
    pub fn file(rel: impl Into<PathBuf>) -> Self {
        Self {
            rel: rel.into(),
            is_dir: false,
            is_symlink: false,
        }
    }

    pub fn dir(rel: impl Into<PathBuf>) -> Self {
        Self {
            rel: rel.into(),
            is_dir: true,
            is_symlink: false,
        }
    }
}

/// Everything the pure planner needs. The `entries` slice is the enumerated
/// source tree; providing it directly keeps `plan` side-effect free and unit
/// testable without a filesystem.
pub struct PlanInput<'a> {
    pub workspace_root: &'a Path,
    pub source_root: &'a Path,
    pub source_id: &'a str,
    pub projection: &'a Projection,
    pub entries: &'a [ScannedEntry],
    /// Resolved workspace env-file names, used by the control-plane guardrail.
    pub env_files: &'a [String],
}

/// Compute the full `LinkPair` set for one projection, applying dest-side and
/// lexical source-side containment. Pure: no filesystem access.
pub fn plan(input: &PlanInput) -> Result<Vec<LinkPair>> {
    let target_abs =
        resolve_target(input.workspace_root, &input.projection.target);

    if !within(input.workspace_root, &target_abs) {
        return Err(ProjectionError::custom(format!(
            "projection target escapes the workspace: {}",
            target_abs.display()
        )));
    }

    let pairs = match input.projection.strategy {
        ProjectionStrategy::Namespaced => {
            plan_namespaced(input.source_root, input.source_id, &target_abs)
        }
        ProjectionStrategy::Mirror => plan_mirror(
            input.source_root,
            input.projection,
            input.entries,
            &target_abs,
        )?,
        ProjectionStrategy::Explicit => plan_explicit(
            input.source_root,
            input.projection,
            input.entries,
            &target_abs,
        )?,
        ProjectionStrategy::Pattern => plan_pattern(
            input.source_root,
            input.projection,
            input.entries,
            &target_abs,
            false,
        )?,
        ProjectionStrategy::Flatten => plan_pattern(
            input.source_root,
            input.projection,
            input.entries,
            &target_abs,
            true,
        )?,
    };

    let guardrails = Guardrails {
        allow_omni_config: input.projection.allow_omni_config,
        allow_git: input.projection.allow_git,
        env_files: input.env_files,
    };

    for pair in &pairs {
        if !within(input.workspace_root, &pair.dest_abs)
            || !within(&target_abs, &pair.dest_abs)
        {
            return Err(ProjectionError::custom(format!(
                "destination escapes the target/workspace: {}",
                pair.dest_abs.display()
            )));
        }
        if !within(input.source_root, &pair.source_abs) {
            return Err(ProjectionError::custom(format!(
                "source escapes the source root: {}",
                pair.source_abs.display()
            )));
        }
        check_dest(&pair.dest_abs, &guardrails)?;
    }

    Ok(pairs)
}

/// Both `@workspace/...` and unrooted targets anchor at the workspace root.
fn resolve_target(workspace_root: &Path, target: &OmniPath) -> PathBuf {
    workspace_root.join(target.unresolved_path()).clean()
}

fn plan_namespaced(
    source_root: &Path,
    source_id: &str,
    target_abs: &Path,
) -> Vec<LinkPair> {
    let mut dest = target_abs.to_path_buf();
    for segment in source_id.split('/') {
        dest.push(segment);
    }

    vec![LinkPair {
        source_abs: source_root.to_path_buf().clean(),
        dest_abs: dest.clean(),
    }]
}

fn plan_mirror(
    source_root: &Path,
    projection: &Projection,
    entries: &[ScannedEntry],
    target_abs: &Path,
) -> Result<Vec<LinkPair>> {
    let scope = match projection.rules.as_slice() {
        [] => None,
        [rule] => Some(build_glob(&rule.r#match)?),
        _ => {
            return Err(ProjectionError::custom(
                "the `mirror` strategy accepts at most one scoping `match` rule",
            ));
        }
    };

    let mut pairs = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let rel_slash = to_slash(&entry.rel);
        if let Some(matcher) = &scope {
            if !matcher.is_match(&rel_slash) {
                continue;
            }
        }
        pairs.push(LinkPair {
            source_abs: source_root.join(&entry.rel).clean(),
            dest_abs: target_abs.join(&entry.rel).clean(),
        });
    }

    Ok(pairs)
}

fn plan_explicit(
    source_root: &Path,
    projection: &Projection,
    entries: &[ScannedEntry],
    target_abs: &Path,
) -> Result<Vec<LinkPair>> {
    let mut pairs = Vec::new();
    for rule in &projection.rules {
        let dest = rule.dest.as_ref().ok_or_else(|| {
            ProjectionError::custom(
                "the `explicit` strategy requires a `dest` on every rule",
            )
        })?;

        let rel = PathBuf::from(&rule.r#match);
        if !entries.iter().any(|e| e.rel == rel) {
            return Err(ProjectionError::custom(format!(
                "explicit source path not found: {}",
                rule.r#match
            )));
        }

        let tail =
            expand_template(&dest_tail(dest), &TemplateVars::from_rel(&rel));
        pairs.push(LinkPair {
            source_abs: source_root.join(&rel).clean(),
            dest_abs: target_abs.join(tail).clean(),
        });
    }

    Ok(pairs)
}

fn plan_pattern(
    source_root: &Path,
    projection: &Projection,
    entries: &[ScannedEntry],
    target_abs: &Path,
    flatten: bool,
) -> Result<Vec<LinkPair>> {
    let mut pairs = Vec::new();
    for rule in &projection.rules {
        let matcher = build_glob(&rule.r#match)?;
        for entry in entries {
            if entry.is_dir {
                continue;
            }
            let rel_slash = to_slash(&entry.rel);
            if !matcher.is_match(&rel_slash) {
                continue;
            }

            let vars = TemplateVars::from_rel(&entry.rel);
            let tail = match (&rule.dest, flatten) {
                (Some(dest), _) => expand_template(&dest_tail(dest), &vars),
                // `flatten` defaults to `{name}` when no `dest` is given.
                (None, true) => expand_template("{name}", &vars),
                (None, false) => {
                    return Err(ProjectionError::custom(
                        "the `pattern` strategy requires a `dest` on every rule",
                    ));
                }
            };

            pairs.push(LinkPair {
                source_abs: source_root.join(&entry.rel).clean(),
                dest_abs: target_abs.join(tail).clean(),
            });
        }
    }

    Ok(pairs)
}

/// The `dest` tail after its `@target` root, ready for template expansion.
fn dest_tail(dest: &DestPath) -> String {
    dest.unresolved_path().to_string_lossy().into_owned()
}

fn build_glob(pattern: &str) -> Result<GlobMatcher> {
    GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|e| {
            ProjectionError::custom(format!("invalid glob `{pattern}`: {e}"))
        })
}

fn to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn within(base: &Path, path: &Path) -> bool {
    path.clean().starts_with(base.clean())
}

struct TemplateVars {
    name: String,
    ext: String,
    basename: String,
    path: String,
    parent: String,
}

impl TemplateVars {
    fn from_rel(rel: &Path) -> Self {
        let os = |s: Option<&std::ffi::OsStr>| {
            s.map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let parent_dir = rel.parent().unwrap_or_else(|| Path::new(""));

        Self {
            name: os(rel.file_stem()),
            ext: os(rel.extension()),
            basename: os(rel.file_name()),
            path: to_slash(parent_dir),
            parent: os(parent_dir.file_name()),
        }
    }
}

fn expand_template(tail: &str, vars: &TemplateVars) -> String {
    tail.replace("{basename}", &vars.basename)
        .replace("{name}", &vars.name)
        .replace("{ext}", &vars.ext)
        .replace("{path}", &vars.path)
        .replace("{parent}", &vars.parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_realpath_contained;

    fn projection(json: &str) -> Projection {
        serde_json::from_str(json).expect("valid projection")
    }

    fn plan_with(
        proj: &Projection,
        source_id: &str,
        entries: &[ScannedEntry],
    ) -> Result<Vec<LinkPair>> {
        plan(&PlanInput {
            workspace_root: Path::new("/ws"),
            source_root: Path::new("/src"),
            source_id,
            projection: proj,
            entries,
            env_files: &[],
        })
    }

    fn pair(source: &str, dest: &str) -> LinkPair {
        LinkPair {
            source_abs: PathBuf::from(source),
            dest_abs: PathBuf::from(dest),
        }
    }

    #[test]
    fn mirror_maps_files_one_to_one_under_target() {
        let proj =
            projection(r#"{"strategy":"mirror","target":"@workspace/dst"}"#);
        let entries = [
            ScannedEntry::file("a.txt"),
            ScannedEntry::dir("sub"),
            ScannedEntry::file("sub/b.txt"),
        ];
        let mut pairs = plan_with(&proj, "id", &entries).unwrap();
        pairs.sort_by(|a, b| a.dest_abs.cmp(&b.dest_abs));
        assert_eq!(
            pairs,
            vec![
                pair("/src/a.txt", "/ws/dst/a.txt"),
                pair("/src/sub/b.txt", "/ws/dst/sub/b.txt"),
            ]
        );
    }

    #[test]
    fn mirror_scopes_by_single_match_rule() {
        let proj = projection(
            r#"{"strategy":"mirror","rules":[{"match":"**/*.txt"}]}"#,
        );
        let entries = [
            ScannedEntry::file("a.txt"),
            ScannedEntry::file("note.md"),
            ScannedEntry::file("sub/b.txt"),
        ];
        let mut pairs = plan_with(&proj, "id", &entries).unwrap();
        pairs.sort_by(|a, b| a.dest_abs.cmp(&b.dest_abs));
        assert_eq!(
            pairs,
            vec![
                pair("/src/a.txt", "/ws/a.txt"),
                pair("/src/sub/b.txt", "/ws/sub/b.txt"),
            ]
        );
    }

    #[test]
    fn explicit_links_literal_paths() {
        let proj = projection(
            r#"{"strategy":"explicit","rules":[{"match":"src/main.rs","dest":"@target/main.rs"}]}"#,
        );
        let entries = [ScannedEntry::file("src/main.rs")];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(pairs, vec![pair("/src/src/main.rs", "/ws/main.rs")]);
    }

    #[test]
    fn explicit_errors_on_missing_source() {
        let proj = projection(
            r#"{"strategy":"explicit","rules":[{"match":"nope.rs","dest":"nope.rs"}]}"#,
        );
        assert!(plan_with(&proj, "id", &[]).is_err());
    }

    #[test]
    fn pattern_routes_with_templates() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"prompts/**/*.md","dest":"rules/{name}.md"}]}"#,
        );
        let entries = [
            ScannedEntry::file("prompts/foo/bar.md"),
            ScannedEntry::file("prompts/top.md"),
            ScannedEntry::file("other.txt"),
        ];
        let mut pairs = plan_with(&proj, "id", &entries).unwrap();
        pairs.sort_by(|a, b| a.dest_abs.cmp(&b.dest_abs));
        assert_eq!(
            pairs,
            vec![
                pair("/src/prompts/foo/bar.md", "/ws/out/rules/bar.md"),
                pair("/src/prompts/top.md", "/ws/out/rules/top.md"),
            ]
        );
    }

    #[test]
    fn flatten_defaults_dest_to_name() {
        let proj = projection(
            r#"{"strategy":"flatten","target":"@workspace/bin","rules":[{"match":"scripts/**/*.sh"}]}"#,
        );
        let entries = [ScannedEntry::file("scripts/a/deploy.sh")];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(
            pairs,
            vec![pair("/src/scripts/a/deploy.sh", "/ws/bin/deploy")]
        );
    }

    #[test]
    fn namespaced_emits_one_nested_dir_link() {
        let proj = projection(
            r#"{"strategy":"namespaced","target":"@workspace/node_modules"}"#,
        );
        let entries = [ScannedEntry::file("whatever.js")];
        let pairs = plan_with(&proj, "@myorg/pkg", &entries).unwrap();
        assert_eq!(pairs, vec![pair("/src", "/ws/node_modules/@myorg/pkg")]);
    }

    #[test]
    fn dest_side_template_escape_is_rejected() {
        let proj = projection(
            r#"{"strategy":"pattern","rules":[{"match":"**/*","dest":"../../{name}"}]}"#,
        );
        let entries = [ScannedEntry::file("a.txt")];
        assert!(plan_with(&proj, "id", &entries).is_err());
    }

    #[test]
    fn lexical_source_escape_is_rejected() {
        let proj = projection(r#"{"strategy":"mirror"}"#);
        let entries = [ScannedEntry::file("../evil")];
        assert!(plan_with(&proj, "id", &entries).is_err());
    }

    #[tokio::test]
    async fn source_realpath_rejects_escaping_symlink() {
        use system_traits::impls::RealSys;

        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, b"x").unwrap();

        let src = tempfile::tempdir().unwrap();
        let inside = src.path().join("inside.txt");
        std::fs::write(&inside, b"y").unwrap();
        let escaping = src.path().join("escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&secret, &escaping).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&secret, &escaping).unwrap();

        let sys = RealSys;
        assert!(
            source_realpath_contained(&sys, src.path(), &inside)
                .await
                .unwrap()
        );
        assert!(
            !source_realpath_contained(&sys, src.path(), &escaping)
                .await
                .unwrap()
        );
    }
}
