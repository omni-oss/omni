//! [`NodePermissions`]: a Tier-1 ([`Tier::PreSpawnFlags`]) backend targeting
//! Node.js's Permission Model (`node --permission`, stable since v22.13 /
//! v23.5).
//!
//! ## Why Node is a deliberately weaker backend
//!
//! Node's permission model is coarser than Deno's, and modelling that honestly
//! is the point — it exercises the crate's coverage/representability machinery.
//! Whatever it cannot express becomes a [`Gap`] (not a hard error), which the
//! orchestrator resolves against the rest of the stack or defers to the
//! configured [`crate::UnenforceablePolicy`]:
//!
//! * **Filesystem** — `--allow-fs-read` / `--allow-fs-write` are **allow-list
//!   only**: there is *no* `--deny-fs-*`, so every `deny` filesystem rule is a
//!   gap. Grants are path/prefix based (a trailing `*` covers a subtree).
//! * **Network** — Node's permission model has **no network flag at all** (it
//!   exposes `--allow-fs-*`, `--allow-child-process`, `--allow-worker`,
//!   `--allow-wasi`, `--allow-addons` — but nothing for sockets). This backend
//!   therefore does **not** cover [`net`](CapabilityDomain::Net): every net rule
//!   is left to another backend (the in-process broker) and the script shim,
//!   and [`require_full_coverage`](crate::require_full_coverage) fails closed if
//!   nothing else covers it. Emitting a `--allow-net` here would be a lie — Node
//!   rejects it as `bad option` — so we never do.
//! * **Child processes** — `--allow-child-process` is likewise all-or-nothing.
//! * **Environment** — Node's permission model does **not** gate environment
//!   variable access at all, so this backend does **not** cover
//!   [`env`](CapabilityDomain::Env); a policy restricting `env` must be covered
//!   by another backend or [`crate::require_full_coverage`] fails closed.

use omni_capabilities::{
    CapabilityAtom, CapabilityDomain, RequiredCapabilities,
};

use crate::lower::{FsScope, classify_fs_glob, validate_flag_value};
use crate::{
    BackendPlan, Coverage, EnforcementBackend, EnforcementError, Gap,
    PatternResolver, Tier,
};

const NAME: &str = "node-permissions";

/// The Node.js Permission Model backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct NodePermissions;

impl EnforcementBackend for NodePermissions {
    fn name(&self) -> &'static str {
        NAME
    }

    fn tier(&self) -> Tier {
        Tier::PreSpawnFlags
    }

    fn coverage(&self) -> Coverage {
        // Node's permission model gates the filesystem and child processes, but
        // has no flag for `net` (sockets) and does not gate `env` at all. Both
        // uncovered domains fall to the broker/shim, and coverage analysis fails
        // closed if a restricting policy has no other backend to enforce them.
        Coverage::of([
            CapabilityDomain::FsRead,
            CapabilityDomain::FsWrite,
            CapabilityDomain::Process,
        ])
    }

    fn plan(
        &self,
        req: &RequiredCapabilities,
        roots: &dyn PatternResolver,
    ) -> Result<BackendPlan, EnforcementError> {
        let mut plan = BackendPlan::new();
        // Turn the permission model on; without it every `--allow-*` is inert
        // and Node runs fully trusted.
        plan.spawn.push_arg("--permission".to_string());

        for &domain in CapabilityDomain::ALL {
            let rules = req.domains().get(&domain);
            let allow = rules.map(|r| r.allow.as_slice()).unwrap_or(&[]);
            let deny = rules.map(|r| r.deny.as_slice()).unwrap_or(&[]);
            let restricted = req.restricted().contains(&domain);

            match domain {
                CapabilityDomain::FsRead => plan_fs(
                    domain,
                    "allow-fs-read",
                    allow,
                    deny,
                    roots,
                    restricted,
                    &mut plan,
                ),
                CapabilityDomain::FsWrite => plan_fs(
                    domain,
                    "allow-fs-write",
                    allow,
                    deny,
                    roots,
                    restricted,
                    &mut plan,
                ),
                CapabilityDomain::Net => {
                    // Not gated by Node's permission model (there is no
                    // `--allow-net`). Coverage excludes it, so a restricting
                    // net policy is enforced by the broker/shim or caught
                    // upstream by coverage analysis — never by a bogus flag.
                }
                CapabilityDomain::Process => plan_boolean(
                    domain,
                    "allow-child-process",
                    "program",
                    allow,
                    deny,
                    restricted,
                    &mut plan,
                ),
                CapabilityDomain::Env => {
                    // Not gated by Node — nothing to contribute. Coverage
                    // excludes it, so a restricting policy is caught upstream.
                }
            }
        }

        Ok(plan)
    }
}

fn gap(
    domain: CapabilityDomain,
    atom: &CapabilityAtom,
    reason: impl Into<String>,
) -> Gap {
    Gap {
        backend: NAME.to_string(),
        domain,
        id: atom.id,
        pattern: atom.pattern.clone(),
        reason: reason.into(),
    }
}

/// Node filesystem permissions: allow-list only (no deny), path/prefix based.
fn plan_fs(
    domain: CapabilityDomain,
    flag: &str,
    allow: &[CapabilityAtom],
    deny: &[CapabilityAtom],
    roots: &dyn PatternResolver,
    restricted: bool,
    plan: &mut BackendPlan,
) {
    // Node has no filesystem deny-list: every deny rule is a gap.
    for atom in deny {
        plan.gaps.push(gap(
            domain,
            atom,
            "Node's permission model has no filesystem deny-list; a `deny` fs \
             rule cannot be enforced (use Deno's `--deny-*` or an in-process \
             broker)",
        ));
    }

    let mut values: Vec<String> = Vec::new();
    for atom in allow {
        let Some(resolved) = roots.resolve(&atom.pattern) else {
            continue; // unregistered root → matches nothing
        };
        match classify_fs_glob(&resolved) {
            // Node grants a subtree via a trailing `*` on the directory.
            Ok(FsScope::Subtree(prefix)) => {
                let value = format!("{prefix}/*");
                if let Err(reason) = validate_flag_value(&value) {
                    plan.gaps.push(gap(domain, atom, reason));
                } else if !values.contains(&value) {
                    values.push(value);
                }
            }
            Ok(FsScope::Exact(path)) => {
                if let Err(reason) = validate_flag_value(&path) {
                    plan.gaps.push(gap(domain, atom, reason));
                } else if !values.contains(&path) {
                    values.push(path);
                }
            }
            Err(reason) => plan.gaps.push(gap(domain, atom, reason)),
        }
    }

    if values.is_empty() {
        if restricted {
            plan.spawn.push_note(format!(
                "{domain}: no allowances granted; denied by default under \
                 --permission (no --{flag} emitted)"
            ));
        }
    } else {
        plan.spawn
            .push_arg(format!("--{flag}={}", values.join(",")));
    }
}

/// Node's all-or-nothing gates (`--allow-net`, `--allow-child-process`): there
/// is no granularity and no deny-list, so only a "grant everything" policy is
/// representable.
fn plan_boolean(
    domain: CapabilityDomain,
    flag: &str,
    selector_noun: &str,
    allow: &[CapabilityAtom],
    deny: &[CapabilityAtom],
    restricted: bool,
    plan: &mut BackendPlan,
) {
    for atom in deny {
        plan.gaps.push(gap(
            domain,
            atom,
            format!(
                "Node's --{flag} is all-or-nothing and has no deny-list; a \
                 `deny` rule cannot be enforced (use Deno or an in-process \
                 broker)"
            ),
        ));
    }

    if allow.is_empty() {
        if restricted {
            plan.spawn.push_note(format!(
                "{domain}: no allowances granted; denied by default under \
                 --permission (no --{flag} emitted)"
            ));
        }
        return;
    }

    // Representable only if some entry means "any" (a bare `*` host/program).
    // Otherwise Node cannot express a specific selector: grant the
    // least-privilege superset it can (the whole gate) as a floor and hand the
    // precise selectors to the script shim, which narrows them per call. Safe:
    // an unresolved gap still fails `build_plan` closed, so this broad floor is
    // only ever applied when a shim covers the rest.
    if allow.iter().any(|a| grants_everything(domain, &a.pattern)) {
        plan.spawn.push_arg(format!("--{flag}"));
    } else {
        plan.spawn.push_arg(format!("--{flag}"));
        // The whole-gate flag is a broad superset of the specific selectors the
        // policy asked for; record it so the planner refuses if no shim/broker
        // narrows this domain per operation.
        plan.superset_domains.insert(domain);
        for atom in allow {
            plan.gaps.push(gap(
                domain,
                atom,
                format!(
                    "Node's --{flag} grants all access or none; it cannot \
                     allow a specific {selector_noun} at launch (granted \
                     broadly and narrowed by the script shim)"
                ),
            ));
        }
    }
}

/// Whether an allow pattern means "grant this whole domain" for Node's boolean
/// gates. Only `process` reaches this now (`net` is uncovered), so a bare `*`
/// program is the only "grant everything" form.
fn grants_everything(_domain: CapabilityDomain, pattern: &str) -> bool {
    pattern == "*"
}

#[cfg(test)]
mod tests {
    use omni_capabilities::{
        CapabilityRules, PathRoots, RequiredCapabilities, Root, project,
    };

    use super::*;
    use crate::{EnforcementBackend, Gap, SpawnPolicy, require_full_coverage};

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

    fn spawn(json: &str) -> SpawnPolicy {
        NodePermissions
            .plan(&require(json), &roots())
            .expect("plan never errors")
            .spawn
    }

    fn gaps(json: &str) -> Vec<Gap> {
        NodePermissions
            .plan(&require(json), &roots())
            .expect("plan never errors")
            .gaps
    }

    #[test]
    fn coverage_excludes_env_and_net() {
        let c = NodePermissions.coverage();
        assert!(c.covers(CapabilityDomain::FsRead));
        assert!(c.covers(CapabilityDomain::FsWrite));
        assert!(c.covers(CapabilityDomain::Process));
        assert!(!c.covers(CapabilityDomain::Env), "Node cannot gate env");
        assert!(
            !c.covers(CapabilityDomain::Net),
            "Node's permission model has no network flag"
        );
    }

    #[test]
    fn never_emits_an_allow_net_flag() {
        // Node has no `--allow-net`; emitting one makes Node abort with
        // `bad option: --allow-net`. Whatever the net policy, we must not.
        for json in [
            r#"[{ "access": "allow", "domain": "net", "patterns": ["*:*"] }]"#,
            r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#,
            r#"[{ "access": "deny",  "domain": "net", "patterns": ["evil.example"] }]"#,
        ] {
            let p = spawn(json);
            assert!(
                !p.args.iter().any(|a| a.contains("net")),
                "Node must never emit a net flag: {p:?}"
            );
        }
    }

    #[test]
    fn net_is_uncovered_so_it_produces_no_gap_here() {
        // Net is not a Node gap (mirroring env): coverage excludes it, so the
        // broker/shim owns it and coverage analysis fails closed elsewhere.
        assert!(
            gaps(
                r#"[{ "access": "allow", "domain": "net", "patterns": ["example.com:443"] }]"#
            )
            .is_empty()
        );
    }

    #[test]
    fn always_enables_the_permission_model() {
        assert!(spawn("[]").args.contains(&"--permission".to_string()));
    }

    #[test]
    fn fs_subtree_uses_trailing_star() {
        let p = spawn(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        assert!(
            p.args.contains(&"--allow-fs-read=/repo/*".to_string()),
            "{p:?}"
        );
    }

    #[test]
    fn fs_exact_file_is_verbatim() {
        let p = spawn(
            r#"[{ "access": "allow", "domain": "fs.write", "patterns": ["@project/out.txt"] }]"#,
        );
        assert!(
            p.args
                .contains(&"--allow-fs-write=/repo/pkg/out.txt".to_string()),
            "{p:?}"
        );
    }

    #[test]
    fn fs_deny_is_a_gap() {
        // Node has no filesystem deny-list at all.
        let gaps = gaps(
            r#"[
                { "access": "allow", "domain": "fs.write", "patterns": ["@workspace/**"] },
                { "access": "deny",  "domain": "fs.write", "patterns": ["@workspace/generated/**"] }
            ]"#,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].pattern, "@workspace/generated/**");
        assert_eq!(gaps[0].domain, CapabilityDomain::FsWrite);
    }

    #[test]
    fn fs_value_with_comma_is_a_gap_not_an_injection() {
        // A resolved path containing a comma would inject an extra allow entry
        // into the comma-joined flag list; it must become a gap instead.
        let roots = PathRoots::new().with(Root::Workspace, "/repo,/etc");
        let req = require(
            r#"[{ "access": "allow", "domain": "fs.read", "patterns": ["@workspace/**"] }]"#,
        );
        let plan = NodePermissions.plan(&req, &roots).expect("plan");
        assert!(
            !plan
                .spawn
                .args
                .iter()
                .any(|a| a.starts_with("--allow-fs-read")),
            "comma value must not be emitted: {:?}",
            plan.spawn.args
        );
        assert_eq!(plan.gaps.len(), 1, "{:?}", plan.gaps);
        assert_eq!(plan.gaps[0].domain, CapabilityDomain::FsRead);
    }

    #[test]
    fn specific_program_is_a_gap() {
        let gaps = gaps(
            r#"[{ "access": "allow", "domain": "process", "patterns": ["git"] }]"#,
        );
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].domain, CapabilityDomain::Process);
    }

    #[test]
    fn allow_any_process_is_the_boolean_flag() {
        let p = spawn(
            r#"[{ "access": "allow", "domain": "process", "patterns": ["*"] }]"#,
        );
        assert!(
            p.args.contains(&"--allow-child-process".to_string()),
            "{p:?}"
        );
    }

    #[test]
    fn uncovered_domain_fails_closed_for_node_alone() {
        // The base `()` profile governs every domain, so both `env` and `net`
        // are restricted; Node covers neither, so coverage must fail closed.
        let req = require("[]");
        let backends: [&dyn EnforcementBackend; 1] = [&NodePermissions];
        let err = require_full_coverage(&req, &backends)
            .expect_err("env and net are uncovered by Node");
        assert_eq!(err.kind(), crate::EnforcementErrorKind::UncoveredDomain);
        let msg = err.to_string();
        assert!(
            msg.contains(&CapabilityDomain::Env.to_string())
                || msg.contains(&CapabilityDomain::Net.to_string()),
            "{err}"
        );
    }
}
