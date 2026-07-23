//! [`DenoFlags`]: a Tier-1 ([`Tier::PreSpawnFlags`]) backend that lowers a
//! policy into Deno's permission flags, replacing the blanket `--allow-all`
//! that `js_runtime` uses today.
//!
//! ## Why Deno maps cleanly (mostly)
//!
//! Deno defaults to **deny** for every permission that is not explicitly
//! granted, which is exactly this crate's fail-closed stance: simply *not*
//! emitting `--allow-all` locks the process down. On top of that, Deno's
//! `--deny-*` flags take precedence over `--allow-*`, mirroring the core
//! model's deny-dominant evaluation. So `DomainRules { allow, deny }` lowers to
//! `--allow-<x>=…` + `--deny-<x>=…` per domain.
//!
//! ## Where it does not, and why we error instead of widening
//!
//! Deno's filesystem permissions are **path-prefix** based, not glob based, and
//! its network/env/run permissions want **literal** values. Our policy patterns
//! are globs (`@workspace/src/**`) and `host:port` selectors. When a pattern
//! cannot be lowered without changing its meaning we return
//! [`EnforcementError::unrepresentable_pattern`] rather than silently granting
//! (or denying) more than intended. In practice this means Deno alone can
//! enforce coarse allow-subtrees, while precise patterns (e.g. `deny **/.git/**`
//! or `net *.example.com`) require the in-process broker tier.

use omni_capabilities::CapabilityAtom;
use omni_capabilities::CapabilityDomain;
use omni_capabilities::RequiredCapabilities;

use crate::lower::{FsScope, classify_fs_glob, has_glob, split_host_port};
use crate::{
    BackendPlan, Coverage, EnforcementBackend, EnforcementError, Gap,
    PatternResolver, Tier,
};

const NAME: &str = "deno-flags";

/// The Deno permission-flags backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenoFlags;

impl EnforcementBackend for DenoFlags {
    fn name(&self) -> &'static str {
        NAME
    }

    fn tier(&self) -> Tier {
        Tier::PreSpawnFlags
    }

    fn coverage(&self) -> Coverage {
        // Deno's permission model spans every domain we model, on every OS it
        // runs on. Whether a *specific pattern* is representable is decided per
        // pattern in `plan`.
        Coverage::all()
    }

    fn plan(
        &self,
        req: &RequiredCapabilities,
        roots: &dyn PatternResolver,
    ) -> Result<BackendPlan, EnforcementError> {
        let mut plan = BackendPlan::new();

        // Deterministic domain order.
        for &domain in CapabilityDomain::ALL {
            let (allow_flag, deny_flag) = deno_flags(domain);
            let rules = req.domains.get(&domain);
            let allow = rules.map(|r| r.allow.as_slice()).unwrap_or(&[]);
            let deny = rules.map(|r| r.deny.as_slice()).unwrap_or(&[]);

            let gaps_before = plan.gaps.len();
            let allow_vals =
                translate_all(domain, allow, roots, &mut plan.gaps);
            let deny_vals = translate_all(domain, deny, roots, &mut plan.gaps);
            let gained_gap = plan.gaps.len() > gaps_before;

            // For a shim-enforceable domain (`net`/`process`) that Deno cannot
            // express precisely, grant the least-privilege superset it can (the
            // bare allow flag) as a floor and let the script shim narrow it per
            // call. Safe: an unresolved gap still fails `build_plan` closed, so
            // the broad floor is only ever applied when a shim covers the rest.
            if gained_gap && is_coarse_shim_domain(domain) {
                if !allow.is_empty() {
                    plan.spawn.push_arg(format!("--{allow_flag}"));
                }
                // Representable denies only tighten, so keep them.
                if !deny_vals.is_empty() {
                    plan.spawn.push_arg(format!(
                        "--{deny_flag}={}",
                        deny_vals.join(",")
                    ));
                }
                continue;
            }

            if allow_vals.is_empty() {
                if req.restricted.contains(&domain) {
                    plan.spawn.push_note(format!(
                        "{domain}: no allowances granted; denied by default \
                         (no --{allow_flag} emitted)"
                    ));
                }
            } else {
                plan.spawn.push_arg(format!(
                    "--{allow_flag}={}",
                    allow_vals.join(",")
                ));
            }

            if !deny_vals.is_empty() {
                plan.spawn
                    .push_arg(format!("--{deny_flag}={}", deny_vals.join(",")));
            }
        }

        // Deno's `node:child_process` compatibility layer reads environment
        // variables (both specific ones like `NODE_V8_COVERAGE` and, when it
        // builds a child env, whatever the script hands it) through Deno's
        // env-permission gate. When the policy permits spawning at all, grant
        // read of exactly the non-sensitive allow-list the script shim passes to
        // a confined child (kept in sync with `INHERITED_ENV_KEYS` in
        // `packages/bridge-rpc-services/.../enforcement/enforced-process.ts`), so
        // an allowed spawn is not blocked by an env-permission error. This never
        // widens `env` to anything sensitive: only these fixed keys are granted.
        let spawns = req
            .domains
            .get(&CapabilityDomain::Process)
            .is_some_and(|r| !r.allow.is_empty());
        if spawns {
            plan.spawn.push_arg(format!(
                "--allow-env={}",
                SPAWN_ENV_ALLOWLIST.join(",")
            ));
        }

        Ok(plan)
    }
}

/// Non-sensitive environment variables a confined child may inherit, granted to
/// Deno so its `node:child_process` layer can build the child's environment.
/// Must stay in sync with `INHERITED_ENV_KEYS` in the TypeScript script shim
/// (`enforcement/enforced-process.ts`).
const SPAWN_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "TERM",
    "TZ",
    "NODE_V8_COVERAGE",
];

/// Whether a domain is one a script-level shim can narrow at runtime (`net` /
/// `process`), so Deno may fall back to a coarse floor + shim rather than a
/// hard gap. Filesystem/env precision is instead resolved by the RPC broker.
fn is_coarse_shim_domain(domain: CapabilityDomain) -> bool {
    matches!(domain, CapabilityDomain::Net | CapabilityDomain::Process)
}

/// `(allow_flag, deny_flag)` names for a domain.
fn deno_flags(domain: CapabilityDomain) -> (&'static str, &'static str) {
    match domain {
        CapabilityDomain::FsRead => ("allow-read", "deny-read"),
        CapabilityDomain::FsWrite => ("allow-write", "deny-write"),
        CapabilityDomain::Net => ("allow-net", "deny-net"),
        CapabilityDomain::Env => ("allow-env", "deny-env"),
        CapabilityDomain::Process => ("allow-run", "deny-run"),
    }
}

/// Translate a pattern list, emitting representable values and recording a
/// [`Gap`] (echoing the atom's opaque id) for each pattern that cannot be
/// represented (best effort).
fn translate_all(
    domain: CapabilityDomain,
    atoms: &[CapabilityAtom],
    roots: &dyn PatternResolver,
    gaps: &mut Vec<Gap>,
) -> Vec<String> {
    let mut out = Vec::new();
    for atom in atoms {
        match translate_pattern(domain, &atom.pattern, roots) {
            Ok(Some(v)) if !out.contains(&v) => out.push(v),
            Ok(_) => {} // duplicate value, or unregistered root → skip
            Err(reason) => gaps.push(Gap {
                backend: NAME.to_string(),
                domain,
                id: atom.id,
                pattern: atom.pattern.clone(),
                reason,
            }),
        }
    }
    out
}

/// Translate one policy pattern into its Deno flag value.
///
/// * `Ok(Some(v))` — the flag value to emit.
/// * `Ok(None)` — the pattern references an unregistered root and therefore
///   matches nothing; contributing nothing is faithful.
/// * `Err(reason)` — the pattern cannot be represented without changing its
///   meaning (a gap).
fn translate_pattern(
    domain: CapabilityDomain,
    pattern: &str,
    roots: &dyn PatternResolver,
) -> Result<Option<String>, String> {
    match domain {
        CapabilityDomain::FsRead | CapabilityDomain::FsWrite => {
            let Some(resolved) = roots.resolve(pattern) else {
                // Unregistered root → matches nothing.
                return Ok(None);
            };
            deno_fs_prefix(&resolved).map(Some)
        }
        CapabilityDomain::Net => deno_net_value(pattern).map(Some),
        CapabilityDomain::Env | CapabilityDomain::Process => {
            deno_literal(pattern).map(Some)
        }
    }
}

/// Lower a resolved filesystem glob into the path prefix Deno grants access to.
///
/// Deno grants a whole subtree under a directory path, which is exactly `/**`
/// semantics, and an exact path for a single file — so both
/// [`FsScope`] variants render to the same string.
fn deno_fs_prefix(glob: &str) -> Result<String, String> {
    Ok(match classify_fs_glob(glob)? {
        FsScope::Subtree(prefix) => prefix,
        FsScope::Exact(path) => path,
    })
}

/// Translate a `host[:port]` pattern into a Deno `--allow-net` value. Deno does
/// not support host wildcards; a `*` in the host is rejected.
fn deno_net_value(pattern: &str) -> Result<String, String> {
    let (host, port) = split_host_port(pattern);
    if has_glob(host) {
        return Err(format!(
            "Deno `--allow-net` does not support host wildcards; `{pattern}` \
             cannot be enforced (use an in-process broker)"
        ));
    }
    Ok(match port {
        // Deno grants all ports for a bare host.
        None | Some("*") => host.to_string(),
        Some(p) => format!("{host}:{p}"),
    })
}

/// Env var names and program names must be literal for Deno.
fn deno_literal(pattern: &str) -> Result<String, String> {
    if has_glob(pattern) {
        return Err(format!(
            "`{pattern}` contains a glob; Deno requires literal names here"
        ));
    }
    Ok(pattern.to_string())
}

#[cfg(test)]
mod tests {
    use omni_capabilities::{CapabilityRules, PathRoots, Root, project};

    use super::*;
    use crate::SpawnPolicy;

    fn roots() -> PathRoots {
        PathRoots::new()
            .with(Root::Workspace, "/repo")
            .with(Root::Project, "/repo/pkg")
    }

    fn require(json: &str) -> RequiredCapabilities {
        let cfg: CapabilityRules =
            serde_json::from_str(json).expect("valid capabilities config");
        project(&cfg, &())
    }

    fn plan(json: &str) -> SpawnPolicy {
        DenoFlags
            .plan(&require(json), &roots())
            .expect("plan never errors")
            .spawn
    }

    fn gaps(json: &str) -> Vec<crate::Gap> {
        DenoFlags
            .plan(&require(json), &roots())
            .expect("plan never errors")
            .gaps
    }

    #[test]
    fn tier_and_full_coverage() {
        assert_eq!(DenoFlags.tier(), Tier::PreSpawnFlags);
        assert_eq!(DenoFlags.coverage(), Coverage::all());
    }

    #[test]
    fn allowing_process_grants_the_spawn_env_allowlist() {
        // A policy that permits spawning must let Deno's node:child_process
        // compat read the non-sensitive env allow-list the shim hands a child,
        // or the child fails to launch.
        let p = plan(
            r#"[{ "access": "allow", "domain": "process", "patterns": ["git"] }]"#,
        );
        let env_flag = p
            .args
            .iter()
            .find(|a| a.starts_with("--allow-env="))
            .expect("an --allow-env grant is emitted when spawning is allowed");
        assert!(env_flag.contains("PATH"), "{env_flag}");
        assert!(env_flag.contains("NODE_V8_COVERAGE"), "{env_flag}");
    }

    #[test]
    fn no_process_means_no_env_grant() {
        // Nothing spawns → no env allowance is emitted at all.
        let p = plan(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        assert!(
            !p.args.iter().any(|a| a.starts_with("--allow-env")),
            "{p:?}"
        );
    }

    #[test]
    fn allow_read_lowers_workspace_subtree_to_prefix() {
        let p = plan(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        assert!(p.args.contains(&"--allow-read=/repo".to_string()), "{p:?}");
    }

    #[test]
    fn exact_file_path_is_kept_verbatim() {
        let p = plan(
            r#"[{ "access": "allow", "domain": "fs.write", "patterns": ["@project/out.txt"] }]"#,
        );
        assert!(
            p.args
                .contains(&"--allow-write=/repo/pkg/out.txt".to_string()),
            "{p:?}"
        );
    }

    #[test]
    fn deny_write_flag_is_emitted() {
        let p = plan(
            r#"[
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["@workspace/generated/**"] }
            ]"#,
        );
        assert!(p.args.contains(&"--allow-write=/repo".to_string()), "{p:?}");
        assert!(
            p.args.contains(&"--deny-write=/repo/generated".to_string()),
            "{p:?}"
        );
    }

    #[test]
    fn net_and_env_and_run_values() {
        let p = plan(
            r#"[
                { "access": "allow", "domain": "net",     "patterns": ["example.com:443", "cache.local:*"] },
                { "access": "allow", "domain": "env",     "patterns": ["HOME", "PATH"] },
                { "access": "allow", "domain": "process", "patterns": ["git"] }
            ]"#,
        );
        assert!(
            p.args.contains(
                &"--allow-net=example.com:443,cache.local".to_string()
            ),
            "{p:?}"
        );
        assert!(
            p.args.contains(&"--allow-env=HOME,PATH".to_string()),
            "{p:?}"
        );
        assert!(p.args.contains(&"--allow-run=git".to_string()), "{p:?}");
    }

    #[test]
    fn empty_policy_emits_no_allow_flags_and_notes_default_deny() {
        // Nothing granted → Deno denies everything; we must NOT emit --allow-*.
        let p = plan("[]");
        assert!(
            p.args.iter().all(|a| !a.starts_with("--allow")),
            "empty policy must not grant anything, got {:?}",
            p.args
        );
        // A note per restricted domain (all five).
        assert_eq!(p.notes.len(), CapabilityDomain::ALL.len());
    }

    #[test]
    fn never_emits_allow_all() {
        let p = plan(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        assert!(
            !p.args.iter().any(|a| a == "--allow-all"),
            "the whole point is to not use --allow-all"
        );
    }

    #[test]
    fn midpath_glob_allow_is_a_gap() {
        let gaps = gaps(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/src/*.rs"] }]"#,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].domain, CapabilityDomain::FsRead);
        assert_eq!(gaps[0].pattern, "@workspace/src/*.rs");
    }

    #[test]
    fn deny_anywhere_glob_is_a_gap() {
        // A classic `deny **/.git/**` cannot be expressed as a Deno path prefix.
        let gaps = gaps(
            r#"[{ "access": "deny", "domain": "fs.write", "patterns": ["**/.git/**"] }]"#,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].pattern, "**/.git/**");
    }

    #[test]
    fn host_wildcard_is_a_gap() {
        let gaps = gaps(
            r#"[{ "access": "allow", "domain": "net", "patterns": ["*.npmjs.org:443"] }]"#,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].domain, CapabilityDomain::Net);
    }

    #[test]
    fn env_glob_allow_is_a_gap() {
        // Deno `--allow-env` takes only literal names, so a globbed env allow
        // cannot be lowered to a flag and must surface as a gap (the in-process
        // broker enforces the glob at runtime instead).
        let gaps = gaps(
            r#"[{ "access": "allow", "domain": "env", "patterns": ["MY_*"] }]"#,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].domain, CapabilityDomain::Env);
        assert_eq!(gaps[0].pattern, "MY_*");
    }

    #[test]
    fn env_glob_deny_is_a_gap() {
        // A globbed env deny is likewise inexpressible as a literal `--deny-env`.
        let gaps = gaps(
            r#"[
                { "access": "allow", "domain": "env", "patterns": ["PATH"] },
                { "access": "deny",  "domain": "env", "patterns": ["*_TOKEN"] }
            ]"#,
        );
        assert!(
            gaps.iter()
                .any(|g| g.domain == CapabilityDomain::Env
                    && g.pattern == "*_TOKEN"),
            "a globbed env deny must be a gap: {gaps:?}"
        );
    }

    #[test]
    fn env_glob_allow_emits_no_allow_env_flag() {
        // env is not a coarse-shim domain, so a gapped glob must NOT be silently
        // widened into any `--allow-env` flag; the domain is left default-denied
        // at launch (and enforced precisely by the broker).
        let p = plan(
            r#"[{ "access": "allow", "domain": "env", "patterns": ["MY_*"] }]"#,
        );
        assert!(
            !p.args.iter().any(|a| a.starts_with("--allow-env")),
            "a globbed env allow must not emit an --allow-env flag: {p:?}"
        );
        // Restricted-but-ungranted → a default-deny note is recorded for env.
        assert!(
            p.notes.iter().any(|n| n.contains("env")),
            "expected a default-deny note for env: {p:?}"
        );
    }

    #[test]
    fn process_glob_is_a_gap() {
        // Program names share the literal-only `deno_literal` path with env, so a
        // globbed `process` allow is also a gap (narrowed by the shim, not Deno).
        let gaps = gaps(
            r#"[{ "access": "allow", "domain": "process", "patterns": ["git-*"] }]"#,
        );
        assert!(
            gaps.iter().any(|g| g.domain == CapabilityDomain::Process
                && g.pattern == "git-*"),
            "a globbed process allow must be a gap: {gaps:?}"
        );
    }

    #[test]
    fn unregistered_root_contributes_nothing() {
        // `@tmp` is not registered → the pattern matches nothing; emitting no
        // allowance is faithful (and is not a gap).
        let bp = DenoFlags
            .plan(
                &require(
                    r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@tmp/**"] }]"#,
                ),
                &roots(),
            )
            .expect("plan never errors");
        assert!(
            bp.spawn.args.iter().all(|a| !a.starts_with("--allow-read")),
            "{bp:?}"
        );
        assert!(bp.gaps.is_empty(), "unregistered root is not a gap: {bp:?}");
    }

    #[test]
    fn args_are_deterministic() {
        let json = r#"[
            { "access": "allow", "domain": "process", "patterns": ["git"] },
            { "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }
        ]"#;
        assert_eq!(plan(json).args, plan(json).args);
        // fs.read precedes process in CapabilityDomain::ALL order.
        let args = plan(json).args;
        let read = args.iter().position(|a| a.starts_with("--allow-read"));
        let run = args.iter().position(|a| a.starts_with("--allow-run"));
        assert!(read < run, "domain order not deterministic: {args:?}");
    }
}
