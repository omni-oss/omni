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
use omni_capabilities::{
    Access, CapabilityRule, DomainRules, PathRoots, Request, Root, rule_matches,
};

use crate::lower::{
    FsScope, classify_fs_glob, has_glob, split_host_port, validate_flag_value,
};
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

        // Set when the `env` policy grants *all* names (a bare `*`), which we
        // lower to Deno's value-less `--allow-env`. It suppresses the
        // spawn-driven bootstrap `--allow-env=<keys>` below, since "all" already
        // subsumes those keys (and Deno rejects a bare and a valued form of the
        // same flag together).
        let mut env_all_allowed = false;

        // Deterministic domain order.
        for &domain in CapabilityDomain::ALL {
            let (allow_flag, deny_flag) = deno_flags(domain);
            let rules = req.domains.get(&domain);
            let allow = rules.map(|r| r.allow.as_slice()).unwrap_or(&[]);
            let deny = rules.map(|r| r.deny.as_slice()).unwrap_or(&[]);

            // Whole-domain wildcard fast path for `env`: a standalone `*` means
            // "all names", which Deno expresses natively as the value-less
            // `--allow-env` / `--deny-env` — a form the per-pattern lowering
            // (`deno_literal`) cannot produce because it treats every glob as
            // unrepresentable. Only `env` needs this here: `process` is a
            // coarse-shim domain already floored to a bare `--allow-run` on any
            // gap (below), `net` has its own host-wildcard handling, and the
            // path-shaped `fs.*` domains have no bare-name wildcard. A *partial*
            // glob (`MY_*`) has no Deno equivalent and correctly falls through
            // to a gap.
            if domain == CapabilityDomain::Env {
                if deny.iter().any(|a| a.pattern == "*") {
                    // Deny-all is dominant and value-less; nothing the allow
                    // side could add survives it.
                    plan.spawn.push_arg(format!("--{deny_flag}"));
                    continue;
                }
                if allow.iter().any(|a| a.pattern == "*") {
                    plan.spawn.push_arg(format!("--{allow_flag}"));
                    env_all_allowed = true;
                    // Representable, non-wildcard denies still tighten the
                    // all-grant (Deno's deny beats allow).
                    let deny_vals =
                        translate_all(domain, deny, roots, &mut plan.gaps);
                    if !deny_vals.is_empty() {
                        plan.spawn.push_arg(format!(
                            "--{deny_flag}={}",
                            deny_vals.join(",")
                        ));
                    }
                    continue;
                }
            }

            let gaps_before = plan.gaps.len();
            let allow_vals =
                translate_all(domain, allow, roots, &mut plan.gaps);
            let deny_vals = translate_all(domain, deny, roots, &mut plan.gaps);
            let gained_gap = plan.gaps.len() > gaps_before;

            // For a shim-enforceable domain (`net`/`process`) that Deno cannot
            // express precisely, grant the least-privilege superset it can (the
            // bare allow flag) as a floor and let the script shim narrow it per
            // call. This is *not* "fail closed": under the default `Warn`
            // strictness the run proceeds. It is safe because the shim covers
            // this domain, so coverage analysis does not fail and the shim —
            // not this broad flag — provides the precise per-call narrowing
            // (`build_plan` still fails closed only if *nothing* covers it).
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
        // read of the non-sensitive bootstrap allow-list the script shim passes
        // to a confined child (kept in sync with `INHERITED_ENV_KEYS` in
        // `packages/bridge-rpc-services/.../enforcement/enforced-process.ts`), so
        // an allowed spawn is not blocked by an env-permission error.
        //
        // The bootstrap keys are non-sensitive by construction (locating
        // binaries, locale, temp dir — never secrets), so they are granted by
        // default. But a `process` allowance must never *override an explicit
        // `env` deny*: any bootstrap key the `env` policy explicitly denies is
        // subtracted from this grant, so `deny env HOME` is honoured even for a
        // generator that is also allowed to spawn.
        let spawns = req
            .domains
            .get(&CapabilityDomain::Process)
            .is_some_and(|r| !r.allow.is_empty());
        if spawns && !env_all_allowed {
            let env_rules = req.domains.get(&CapabilityDomain::Env);
            let granted: Vec<&str> = SPAWN_ENV_ALLOWLIST
                .iter()
                .copied()
                .filter(|key| {
                    !env_rules.is_some_and(|r| env_explicitly_denied(r, key))
                })
                .collect();
            if !granted.is_empty() {
                plan.spawn
                    .push_arg(format!("--allow-env={}", granted.join(",")));
            }
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
            // Guard against injecting extra list entries: a representable value
            // that embeds a `,` / `=` / control char is not safely joinable
            // into `--flag=a,b,c`, so treat it as a gap (broker enforces it).
            Ok(Some(v)) => match validate_flag_value(&v) {
                Ok(()) if !out.contains(&v) => out.push(v),
                Ok(()) => {} // duplicate value → skip
                Err(reason) => gaps.push(Gap {
                    backend: NAME.to_string(),
                    domain,
                    id: atom.id,
                    pattern: atom.pattern.clone(),
                    reason,
                }),
            },
            Ok(None) => {} // unregistered root → skip
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

/// Whether the merged `env` policy carries an explicit `deny` rule matching
/// `name`. Env deny patterns are matched as plain globs (so `*_TOKEN` matches),
/// mirroring the broker/shim. Used to subtract an explicitly-denied key from the
/// spawn-driven `--allow-env` bootstrap grant, so a `process` allowance can
/// never override an explicit `env` deny — while the fixed, non-sensitive
/// bootstrap keys the policy does not mention stay granted so ordinary spawns
/// keep working.
fn env_explicitly_denied(rules: &DomainRules, name: &str) -> bool {
    if rules.deny.is_empty() {
        return false;
    }
    // `rule_matches` ignores `roots` for the `env` domain (names are not paths)
    // and ignores `access` entirely, so an empty root set is fine here.
    let rule = CapabilityRule {
        access: Access::Deny,
        domain: CapabilityDomain::Env,
        patterns: rules.deny.iter().map(|a| a.pattern.clone()).collect(),
        on_unenforceable: None,
    };
    rule_matches(&rule, &Request::Env { name }, &PathRoots::<Root>::new())
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
    fn process_allow_grants_the_bootstrap_env_by_default() {
        // Non-breaking default: when the `env` policy does not deny them, the
        // fixed non-sensitive bootstrap keys are granted so ordinary spawns
        // (which need PATH etc. to locate their binary) keep working.
        let p = plan(
            r#"[{ "access": "allow", "domain": "process", "patterns": ["git"] }]"#,
        );
        let env_flag = p
            .args
            .iter()
            .find(|a| a.starts_with("--allow-env="))
            .expect(
                "a bootstrap env grant is emitted when spawning is allowed",
            );
        assert!(env_flag.contains("PATH"), "{env_flag}");
        assert!(env_flag.contains("NODE_V8_COVERAGE"), "{env_flag}");
    }

    #[test]
    fn an_explicit_env_deny_is_subtracted_from_the_bootstrap_grant() {
        // A `process` allowance must never override an explicit `env` deny: a
        // denied bootstrap key (HOME) is removed from the --allow-env grant,
        // while the other non-sensitive keys remain.
        let p = plan(
            r#"[
                { "access": "allow", "domain": "process", "patterns": ["git"] },
                { "access": "deny",  "domain": "env",     "patterns": ["HOME"] }
            ]"#,
        );
        let env_flag = p
            .args
            .iter()
            .find(|a| a.starts_with("--allow-env="))
            .expect("the bootstrap env grant is still emitted");
        assert!(env_flag.contains("PATH"), "{env_flag}");
        assert!(
            !env_flag.contains("HOME"),
            "an explicitly denied env key must not be granted via the process \
             floor: {env_flag}"
        );
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
    fn env_allow_all_wildcard_lowers_to_the_bare_allow_env_flag() {
        // A whole-domain `*` means "all env names", which Deno expresses as the
        // value-less `--allow-env` (NOT a gap, and NOT a per-name list). This is
        // what lets Deno's node-compat `execSync` (which reads the whole env to
        // spawn) work under an explicit allow-all policy.
        let src =
            r#"[{ "access": "allow", "domain": "env", "patterns": ["*"] }]"#;
        let p = plan(src);
        assert!(
            p.args.contains(&"--allow-env".to_string()),
            "the bare all-env flag must be emitted: {p:?}"
        );
        assert!(
            !p.args.iter().any(|a| a.starts_with("--allow-env=")),
            "the valued form must not also be emitted: {p:?}"
        );
        assert!(
            gaps(src).is_empty(),
            "a representable all-grant must not be reported as a gap"
        );
    }

    #[test]
    fn env_allow_all_still_honours_a_specific_deny() {
        // `allow *` + `deny SECRET` → all env except SECRET (Deno's deny beats
        // allow), so both flags are emitted.
        let p = plan(
            r#"[
                { "access": "allow", "domain": "env", "patterns": ["*"] },
                { "access": "deny",  "domain": "env", "patterns": ["SECRET"] }
            ]"#,
        );
        assert!(p.args.contains(&"--allow-env".to_string()), "{p:?}");
        assert!(p.args.contains(&"--deny-env=SECRET".to_string()), "{p:?}");
    }

    #[test]
    fn env_deny_all_wildcard_lowers_to_the_bare_deny_env_flag() {
        // A whole-domain `*` deny means "deny all env", the value-less
        // `--deny-env`, and dominates any allow.
        let p = plan(
            r#"[{ "access": "deny", "domain": "env", "patterns": ["*"] }]"#,
        );
        assert!(p.args.contains(&"--deny-env".to_string()), "{p:?}");
    }

    #[test]
    fn env_allow_all_suppresses_the_bootstrap_env_grant_when_spawning() {
        // With `allow env *` the bare `--allow-env` already grants everything,
        // so the spawn-driven bootstrap `--allow-env=<keys>` must NOT also be
        // emitted (Deno rejects a bare and a valued form of the same flag).
        let p = plan(
            r#"[
                { "access": "allow", "domain": "process", "patterns": ["git"] },
                { "access": "allow", "domain": "env",     "patterns": ["*"] }
            ]"#,
        );
        assert!(p.args.contains(&"--allow-env".to_string()), "{p:?}");
        assert!(
            !p.args.iter().any(|a| a.starts_with("--allow-env=")),
            "the bootstrap valued grant must be suppressed by the all-grant: {p:?}"
        );
    }

    #[test]
    fn env_partial_glob_is_still_a_gap() {
        // Only a standalone `*` maps to the bare flag; a partial glob has no
        // Deno equivalent and must remain a gap (broker/shim enforced).
        let g = gaps(
            r#"[{ "access": "allow", "domain": "env", "patterns": ["MY_*"] }]"#,
        );
        assert!(
            g.iter()
                .any(|g| g.domain == CapabilityDomain::Env
                    && g.pattern == "MY_*"),
            "a partial env glob must gap: {g:?}"
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
    fn net_host_with_comma_is_a_gap_not_an_injection() {
        // A host embedding a comma would inject a second allow entry into the
        // joined `--allow-net=a,b` list; it must become a gap instead.
        let bp = DenoFlags
            .plan(
                &require(
                    r#"[{ "access": "allow", "domain": "net", "patterns": ["good.example,evil.example"] }]"#,
                ),
                &roots(),
            )
            .expect("plan never errors");
        assert!(
            !bp.spawn.args.iter().any(|a| a.starts_with("--allow-net=")),
            "comma host must not be emitted as a valued flag: {:?}",
            bp.spawn.args
        );
        assert_eq!(bp.gaps.len(), 1, "{:?}", bp.gaps);
        assert_eq!(bp.gaps[0].domain, CapabilityDomain::Net);
    }

    #[test]
    fn fs_path_with_comma_is_a_gap_not_an_injection() {
        // A resolved root containing a comma must not widen the allow list.
        let roots = PathRoots::new().with(Root::Workspace, "/repo,/etc");
        let bp = DenoFlags
            .plan(
                &require(
                    r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
                ),
                &roots,
            )
            .expect("plan never errors");
        assert!(
            !bp.spawn.args.iter().any(|a| a.starts_with("--allow-read")),
            "comma path must not be emitted: {:?}",
            bp.spawn.args
        );
        assert_eq!(bp.gaps.len(), 1, "{:?}", bp.gaps);
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
