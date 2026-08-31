use std::path::{Path, PathBuf};

use omni_glob::{GlobMatcher, GlobOptions};
use omni_projection_configurations::{
    DestPath, MatchKind, OmniPath, Projection,
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

/// A candidate link plus whether it links a whole directory (as opposed to a
/// single file). The flag drives shallowest-wins pruning, the directory
/// contents guardrail, and the run-wide nested-collision check.
pub struct PlannedPair {
    pub pair: LinkPair,
    pub is_dir_link: bool,
}

/// Compute the full `LinkPair` set for one projection, applying dest-side and
/// lexical source-side containment. Pure: no filesystem access.
pub fn plan(input: &PlanInput) -> Result<Vec<PlannedPair>> {
    let common = input.projection.common();
    let target_abs = resolve_target(input.workspace_root, &common.target);

    if !within(input.workspace_root, &target_abs) {
        return Err(ProjectionError::custom(format!(
            "projection target escapes the workspace: {}",
            target_abs.display()
        )));
    }

    let mut candidates = match input.projection {
        Projection::Namespaced(_) => vec![PlannedPair {
            pair: namespaced_pair(
                input.source_root,
                input.source_id,
                &target_abs,
            ),
            is_dir_link: true,
        }],
        Projection::Mirror(p) => {
            let scope = p.scope.as_ref().map(|s| s.to_vec());
            plan_mirror(
                input.source_root,
                scope.as_deref(),
                input.entries,
                &target_abs,
            )?
        }
        Projection::Explicit(p) => plan_explicit(
            input.source_root,
            &p.rules,
            input.entries,
            &target_abs,
        )?,
        Projection::Pattern(p) => plan_pattern(
            input.source_root,
            &p.rules,
            input.entries,
            &target_abs,
        )?,
        Projection::Flatten(p) => plan_flatten(
            input.source_root,
            &p.rules,
            input.entries,
            &target_abs,
        )?,
    };

    prune_nested(&mut candidates);

    let guardrails = Guardrails {
        allow_omni_config: common.allow_omni_config,
        allow_git: common.allow_git,
        env_files: input.env_files,
    };

    let mut pairs = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let pair = &candidate.pair;
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

        if candidate.is_dir_link {
            check_dir_contents(
                input.source_root,
                input.entries,
                pair,
                &guardrails,
            )?;
        }

        pairs.push(candidate);
    }

    Ok(pairs)
}

/// Both `@workspace/...` and unrooted targets anchor at the workspace root.
fn resolve_target(workspace_root: &Path, target: &OmniPath) -> PathBuf {
    workspace_root.join(target.unresolved_path()).clean()
}

fn namespaced_pair(
    source_root: &Path,
    source_id: &str,
    target_abs: &Path,
) -> LinkPair {
    let mut dest = target_abs.to_path_buf();
    for segment in source_id.split('/') {
        dest.push(segment);
    }

    LinkPair {
        source_abs: source_root.to_path_buf().clean(),
        dest_abs: dest.clean(),
    }
}

fn plan_mirror(
    source_root: &Path,
    scope: Option<&[String]>,
    entries: &[ScannedEntry],
    target_abs: &Path,
) -> Result<Vec<PlannedPair>> {
    let matcher = scope.map(build_matcher).transpose()?;

    let mut candidates = Vec::new();
    for entry in entries {
        if entry.is_dir {
            continue;
        }
        let rel_slash = to_slash(&entry.rel);
        if let Some(matcher) = &matcher {
            if !matcher.is_match(&rel_slash) {
                continue;
            }
        }
        candidates.push(PlannedPair {
            pair: LinkPair {
                source_abs: source_root.join(&entry.rel).clean(),
                dest_abs: target_abs.join(&entry.rel).clean(),
            },
            is_dir_link: false,
        });
    }

    Ok(candidates)
}

fn plan_explicit(
    source_root: &Path,
    rules: &[omni_projection_configurations::ExplicitRule],
    entries: &[ScannedEntry],
    target_abs: &Path,
) -> Result<Vec<PlannedPair>> {
    let mut candidates = Vec::new();
    for rule in rules {
        let rel = PathBuf::from(&rule.source);
        let entry = entries.iter().find(|e| e.rel == rel).ok_or_else(|| {
            ProjectionError::custom(format!(
                "explicit source path not found: {}",
                rule.source
            ))
        })?;

        let tail = expand_template(
            &dest_tail(&rule.dest),
            &TemplateVars::from_rel(&rel),
        );
        candidates.push(PlannedPair {
            pair: LinkPair {
                source_abs: source_root.join(&rel).clean(),
                dest_abs: target_abs.join(tail).clean(),
            },
            is_dir_link: entry.is_dir,
        });
    }

    Ok(candidates)
}

fn plan_pattern(
    source_root: &Path,
    rules: &[omni_projection_configurations::PatternRule],
    entries: &[ScannedEntry],
    target_abs: &Path,
) -> Result<Vec<PlannedPair>> {
    let mut candidates = Vec::new();
    for rule in rules {
        let matcher = build_matcher(&rule.r#match.to_vec())?;
        for entry in matching_entries(entries, &matcher, rule.match_kind) {
            let vars = TemplateVars::from_rel(&entry.rel);
            let tail = expand_template(&dest_tail(&rule.dest), &vars);
            candidates.push(candidate(
                source_root,
                target_abs,
                &entry.rel,
                tail,
                rule.match_kind,
            ));
        }
    }

    Ok(candidates)
}

fn plan_flatten(
    source_root: &Path,
    rules: &[omni_projection_configurations::FlattenRule],
    entries: &[ScannedEntry],
    target_abs: &Path,
) -> Result<Vec<PlannedPair>> {
    let mut candidates = Vec::new();
    for rule in rules {
        let matcher = build_matcher(&rule.r#match.to_vec())?;
        for entry in matching_entries(entries, &matcher, rule.match_kind) {
            let vars = TemplateVars::from_rel(&entry.rel);
            let tail = match &rule.dest {
                Some(dest) => expand_template(&dest_tail(dest), &vars),
                // A directory has no meaningful stem/ext split, so its default
                // is the whole basename; a file defaults to its stem.
                None => match rule.match_kind {
                    MatchKind::Dir => expand_template("{basename}", &vars),
                    MatchKind::File => expand_template("{name}", &vars),
                },
            };
            candidates.push(candidate(
                source_root,
                target_abs,
                &entry.rel,
                tail,
                rule.match_kind,
            ));
        }
    }

    Ok(candidates)
}

/// Entries a glob selects, filtered to the kind (file vs. directory) the rule
/// targets.
fn matching_entries<'a>(
    entries: &'a [ScannedEntry],
    matcher: &'a GlobMatcher,
    match_kind: MatchKind,
) -> impl Iterator<Item = &'a ScannedEntry> {
    let want_dir = matches!(match_kind, MatchKind::Dir);
    entries.iter().filter(move |entry| {
        entry.is_dir == want_dir && matcher.is_match(to_slash(&entry.rel))
    })
}

fn candidate(
    source_root: &Path,
    target_abs: &Path,
    rel: &Path,
    tail: String,
    match_kind: MatchKind,
) -> PlannedPair {
    PlannedPair {
        pair: LinkPair {
            source_abs: source_root.join(rel).clean(),
            dest_abs: target_abs.join(tail).clean(),
        },
        is_dir_link: matches!(match_kind, MatchKind::Dir),
    }
}

/// Drop any candidate whose destination is a strict descendant of a
/// directory-link destination: writing through a directory link would land back
/// inside the source. Identical destinations stay and are resolved by the
/// run-wide collision check. Shallowest directory link wins.
fn prune_nested(candidates: &mut Vec<PlannedPair>) {
    let dir_dests: Vec<PathBuf> = candidates
        .iter()
        .filter(|c| c.is_dir_link)
        .map(|c| c.pair.dest_abs.clone())
        .collect();

    candidates.retain(|c| {
        !dir_dests.iter().any(|dir| {
            c.pair.dest_abs != *dir && c.pair.dest_abs.starts_with(dir)
        })
    });
}

/// Run the control-plane / `.git` refusal over the non-symlinked entries beneath
/// a directory link. The scan does not descend symlinked subdirectories, so this
/// sees exactly the subtree configuration discovery could reach through a
/// copy-kind link.
fn check_dir_contents(
    source_root: &Path,
    entries: &[ScannedEntry],
    pair: &LinkPair,
    guardrails: &Guardrails,
) -> Result<()> {
    let Ok(dir_rel) = pair.source_abs.strip_prefix(source_root) else {
        return Ok(());
    };

    for entry in entries {
        if entry.is_symlink {
            continue;
        }
        let Ok(sub) = entry.rel.strip_prefix(dir_rel) else {
            continue;
        };
        if sub.as_os_str().is_empty() {
            continue;
        }
        check_dest(&pair.dest_abs.join(sub), guardrails)?;
    }

    Ok(())
}

/// The `dest` tail after its `@target` root, ready for template expansion.
fn dest_tail(dest: &DestPath) -> String {
    dest.unresolved_path().to_string_lossy().into_owned()
}

fn build_matcher(patterns: &[String]) -> Result<GlobMatcher> {
    GlobMatcher::new(
        patterns,
        GlobOptions {
            literal_separator: true,
        },
    )
    .map_err(|e| {
        ProjectionError::custom(format!("invalid glob in {patterns:?}: {e}"))
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
        .map(|planned| planned.into_iter().map(|p| p.pair).collect())
    }

    fn pair(source: &str, dest: &str) -> LinkPair {
        LinkPair {
            source_abs: PathBuf::from(source),
            dest_abs: PathBuf::from(dest),
        }
    }

    fn plan_planned(
        proj: &Projection,
        source_id: &str,
        entries: &[ScannedEntry],
    ) -> Result<Vec<PlannedPair>> {
        plan(&PlanInput {
            workspace_root: Path::new("/ws"),
            source_root: Path::new("/src"),
            source_id,
            projection: proj,
            entries,
            env_files: &[],
        })
    }

    #[test]
    fn plan_reports_is_dir_link_per_pair() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"d","match_kind":"dir","dest":"@target/d"},{"match":"f.txt","dest":"@target/f.txt"}]}"#,
        );
        let entries = [ScannedEntry::dir("d"), ScannedEntry::file("f.txt")];
        let planned = plan_planned(&proj, "id", &entries).unwrap();

        let dir = planned
            .iter()
            .find(|p| p.pair.dest_abs.ends_with("d"))
            .expect("dir link present");
        assert!(dir.is_dir_link, "a dir-kind match is a directory link");

        let file = planned
            .iter()
            .find(|p| p.pair.dest_abs.ends_with("f.txt"))
            .expect("file link present");
        assert!(
            !file.is_dir_link,
            "a file-kind match is not a directory link"
        );
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
    fn mirror_scopes_by_scope_glob() {
        let proj = projection(r#"{"strategy":"mirror","scope":"**/*.txt"}"#);
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
            r#"{"strategy":"explicit","rules":[{"source":"src/main.rs","dest":"@target/main.rs"}]}"#,
        );
        let entries = [ScannedEntry::file("src/main.rs")];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(pairs, vec![pair("/src/src/main.rs", "/ws/main.rs")]);
    }

    #[test]
    fn pattern_match_list_is_an_or_union() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":["prompts/**/*.md","docs/**/*.md"],"dest":"{name}.md"}]}"#,
        );
        let entries = [
            ScannedEntry::file("prompts/a.md"),
            ScannedEntry::file("docs/b.md"),
            ScannedEntry::file("other/c.md"),
        ];
        let mut pairs = plan_with(&proj, "id", &entries).unwrap();
        pairs.sort_by(|a, b| a.dest_abs.cmp(&b.dest_abs));
        assert_eq!(
            pairs,
            vec![
                pair("/src/prompts/a.md", "/ws/out/a.md"),
                pair("/src/docs/b.md", "/ws/out/b.md"),
            ]
        );
    }

    #[test]
    fn pattern_entry_matched_by_two_includes_yields_one_link() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":["**/*.md","prompts/*.md"],"dest":"{name}.md"}]}"#,
        );
        let entries = [ScannedEntry::file("prompts/a.md")];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(pairs, vec![pair("/src/prompts/a.md", "/ws/out/a.md")]);
    }

    #[test]
    fn pattern_match_list_excludes_win() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":["**/*.md","!secret/**"],"dest":"{name}.md"}]}"#,
        );
        let entries = [
            ScannedEntry::file("a.md"),
            ScannedEntry::file("secret/b.md"),
        ];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(pairs, vec![pair("/src/a.md", "/ws/out/a.md")]);
    }

    #[test]
    fn mirror_scope_list_includes_and_excludes() {
        let proj =
            projection(r#"{"strategy":"mirror","scope":["**","!drafts/**"]}"#);
        let entries = [
            ScannedEntry::file("keep.txt"),
            ScannedEntry::file("nested/keep.txt"),
            ScannedEntry::file("drafts/skip.txt"),
        ];
        let mut pairs = plan_with(&proj, "id", &entries).unwrap();
        pairs.sort_by(|a, b| a.dest_abs.cmp(&b.dest_abs));
        assert_eq!(
            pairs,
            vec![
                pair("/src/keep.txt", "/ws/keep.txt"),
                pair("/src/nested/keep.txt", "/ws/nested/keep.txt"),
            ]
        );
    }

    #[test]
    fn explicit_errors_on_missing_source() {
        let proj = projection(
            r#"{"strategy":"explicit","rules":[{"source":"nope.rs","dest":"nope.rs"}]}"#,
        );
        assert!(plan_with(&proj, "id", &[]).is_err());
    }

    #[test]
    fn explicit_links_a_literal_directory() {
        let proj = projection(
            r#"{"strategy":"explicit","rules":[{"source":"pkg","dest":"@target/pkg"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("pkg"),
            ScannedEntry::file("pkg/inner.txt"),
        ];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(pairs, vec![pair("/src/pkg", "/ws/pkg")]);
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

    // ── Directory-aware pattern / flatten ────────────────────────────────────

    #[test]
    fn pattern_dir_links_each_matched_directory() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/.agents/skills","rules":[{"match":"skills/engineering/*","match_kind":"dir","dest":"@target/{basename}"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("skills"),
            ScannedEntry::dir("skills/engineering"),
            ScannedEntry::dir("skills/engineering/tdd"),
            ScannedEntry::file("skills/engineering/tdd/SKILL.md"),
            ScannedEntry::dir("skills/engineering/code-review"),
            ScannedEntry::file("skills/engineering/code-review/SKILL.md"),
        ];
        let mut pairs = plan_with(&proj, "id", &entries).unwrap();
        pairs.sort_by(|a, b| a.dest_abs.cmp(&b.dest_abs));
        assert_eq!(
            pairs,
            vec![
                pair(
                    "/src/skills/engineering/code-review",
                    "/ws/.agents/skills/code-review"
                ),
                pair("/src/skills/engineering/tdd", "/ws/.agents/skills/tdd"),
            ]
        );
    }

    #[test]
    fn flatten_dir_defaults_to_basename_preserving_dots() {
        let proj = projection(
            r#"{"strategy":"flatten","target":"@workspace/plugins","rules":[{"match":"vendor/*","match_kind":"dir"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("vendor"),
            ScannedEntry::dir("vendor/my.plugin"),
            ScannedEntry::file("vendor/my.plugin/index.js"),
        ];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(
            pairs,
            vec![pair("/src/vendor/my.plugin", "/ws/plugins/my.plugin")]
        );
    }

    #[test]
    fn recursive_dir_glob_collapses_to_shallowest() {
        // A structure-preserving dest makes `pkgs/a/nested` land inside
        // `pkgs/a`, so the nested directory link is pruned in favor of the
        // shallowest one.
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"pkgs/**","match_kind":"dir","dest":"@target/{path}/{basename}"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("pkgs"),
            ScannedEntry::dir("pkgs/a"),
            ScannedEntry::dir("pkgs/a/nested"),
        ];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(pairs, vec![pair("/src/pkgs/a", "/ws/out/pkgs/a")]);
    }

    #[test]
    fn nested_dest_pair_is_pruned_under_dir_link() {
        // A dir link to out/pkg and a file route landing inside out/pkg/... :
        // the file pair is dropped because it would write back through the link.
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"pkg","match_kind":"dir","dest":"@target/pkg"},{"match":"pkg/**/*.md","dest":"pkg/{name}.md"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("pkg"),
            ScannedEntry::file("pkg/readme.md"),
        ];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(pairs, vec![pair("/src/pkg", "/ws/out/pkg")]);
    }

    #[test]
    fn identical_dir_dests_remain_a_collision() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"a","match_kind":"dir","dest":"@target/same"},{"match":"b","match_kind":"dir","dest":"@target/same"}]}"#,
        );
        let entries = [ScannedEntry::dir("a"), ScannedEntry::dir("b")];
        let pairs = plan_with(&proj, "id", &entries).unwrap();
        assert_eq!(pairs.len(), 2, "identical dests are kept as a collision");
    }

    #[test]
    fn dir_link_containing_control_plane_config_is_refused() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"pkg","match_kind":"dir","dest":"@target/pkg"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("pkg"),
            ScannedEntry::file("pkg/project.omni.yaml"),
        ];
        assert!(
            plan_with(&proj, "id", &entries).is_err(),
            "a dir link whose contents include a control-plane manifest is refused"
        );
    }

    #[test]
    fn dir_link_control_plane_allowed_with_flag() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","allow_omni_config":true,"rules":[{"match":"pkg","match_kind":"dir","dest":"@target/pkg"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("pkg"),
            ScannedEntry::file("pkg/project.omni.yaml"),
        ];
        assert!(plan_with(&proj, "id", &entries).is_ok());
    }

    #[test]
    fn dir_link_git_contents_are_refused() {
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"pkg","match_kind":"dir","dest":"@target/pkg"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("pkg"),
            ScannedEntry::dir("pkg/.git"),
            ScannedEntry::file("pkg/.git/config"),
        ];
        assert!(plan_with(&proj, "id", &entries).is_err());
    }

    #[test]
    fn dir_link_ignores_symlinked_contents_for_guardrail() {
        // A symlinked manifest beneath the dir is not part of the subtree
        // discovery could reach, so it does not trip the guardrail.
        let proj = projection(
            r#"{"strategy":"pattern","target":"@workspace/out","rules":[{"match":"pkg","match_kind":"dir","dest":"@target/pkg"}]}"#,
        );
        let entries = [
            ScannedEntry::dir("pkg"),
            ScannedEntry {
                rel: PathBuf::from("pkg/project.omni.yaml"),
                is_dir: false,
                is_symlink: true,
            },
        ];
        assert!(plan_with(&proj, "id", &entries).is_ok());
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
