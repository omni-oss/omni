//! Selection and detection of the JavaScript runtime used to launch a bridge
//! service process.

/// Which JavaScript runtime to launch a bridge service with.
///
/// [`Auto`](Self::Auto) defers the choice to [`resolve`](Self::resolve), which
/// probes `PATH` for a supported runtime.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DelegatingJsRuntimeOption {
    Deno,
    Node,
    Bun,
    #[default]
    Auto,
}

impl DelegatingJsRuntimeOption {
    /// Resolves [`Auto`](Self::Auto) to a concrete runtime detected on `PATH`.
    /// Concrete variants are returned unchanged. Returns `None` only when
    /// `Auto` is requested and no runtime is found.
    pub fn resolve(self) -> Option<DelegatingJsRuntimeOption> {
        match self {
            DelegatingJsRuntimeOption::Auto => auto_detect_runtime_option(),
            concrete => Some(concrete),
        }
    }
}

/// Detects the first supported JavaScript runtime available on `PATH`, in
/// preference order `deno` → `node` → `bun`.
///
/// The order prefers the **most confinable** runtime first: Deno and Node both
/// expose a pre-spawn permission model omni can lower a capability policy into,
/// whereas Bun has none — so an auto-resolved Bun cannot confine the always
/// fail-closed `process` domain and would be refused. Preferring Deno/Node keeps
/// auto-detection working under mandatory enforcement; an explicit `runtime: bun`
/// still opts in (and fails closed if its policy is not enforceable).
pub fn auto_detect_runtime_option() -> Option<DelegatingJsRuntimeOption> {
    Some(if which::which("deno").is_ok() {
        DelegatingJsRuntimeOption::Deno
    } else if which::which("node").is_ok() {
        DelegatingJsRuntimeOption::Node
    } else if which::which("bun").is_ok() {
        DelegatingJsRuntimeOption::Bun
    } else {
        return None;
    })
}

/// The minimum Node.js major version omni targets for capability-confined
/// generators: the release from which **every** permission-model allowance omni
/// lowers a policy into is available.
///
/// Node grew the permission model incrementally — `--permission` and the
/// filesystem / child-process allowances stabilized in v22.13, while network
/// permissions (`--allow-net`) arrived in v24. v24 is therefore the first
/// release that can confine *all* of omni's domains (fs, process, net), so it is
/// the baseline: below it, a policy that restricts `net` cannot be enforced by
/// Node's launch flags.
pub const MIN_SUPPORTED_NODE_MAJOR: u32 = 24;

/// The `(major, minor, patch)` version of the `node` that would actually be
/// launched, parsed from `node --version` (e.g. `v26.5.0`). Uses the same
/// bare-name resolution and inherited environment as the real spawn, so a
/// version-manager shim resolves to the same runtime. `None` if node could not
/// be run or its output could not be parsed.
pub fn node_version() -> Option<(u32, u32, u32)> {
    let output = std::process::Command::new("node")
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    // The last line is the version (`vMAJOR.MINOR.PATCH`); any preceding lines
    // are shim/tool noise on stdout.
    let line = raw
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('v'))?;
    let mut parts = line.trim().trim_start_matches('v').split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// Whether the resolved `node` can confine the `net` domain via `--allow-net`.
///
/// Primary check is the version baseline ([`MIN_SUPPORTED_NODE_MAJOR`]); if the
/// version cannot be parsed (unusual shim output), it falls back to directly
/// feature-detecting the flag in `node --help`. Fail-open only when *both* are
/// inconclusive, since the runner's fail-fast child-exit path is the backstop.
pub fn node_supports_net() -> bool {
    if let Some((major, _, _)) = node_version() {
        return major >= MIN_SUPPORTED_NODE_MAJOR
            || node_advertises_flag("--allow-net");
    }
    node_advertises_flag("--allow-net")
}

/// Best-effort check that the `node` binary which would actually be launched
/// advertises `flag` (e.g. `"--allow-net"`) in its `--help` output.
///
/// Used as a fallback when [`node_version`] cannot be parsed. This probe uses the
/// **same bare-name resolution and inherited environment** as the real spawn
/// (`node --help`), so it reflects what that launch would see — including any
/// version-manager shim indirection.
///
/// It is deliberately fail-open: if `--help` cannot be run or read, it returns
/// `true` ("assume supported") so an inconclusive probe never blocks a launch
/// that might succeed.
pub fn node_advertises_flag(flag: &str) -> bool {
    let Ok(output) = std::process::Command::new("node").arg("--help").output()
    else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    // `--help` lists the flag by name; match tolerantly on the name without its
    // leading dashes.
    let needle = flag.trim_start_matches('-');
    String::from_utf8_lossy(&output.stdout).contains(needle)
}

/// The absolute path of the binary the given runtime *actually* runs from, as
/// reported by the runtime itself.
///
/// A version-manager shim on `PATH` (nub, nvm, fnm, volta, …) is a launcher that
/// re-execs the real runtime living elsewhere; neither `which` nor
/// `canonicalize` reveals that target because the shim is not a symlink to it.
/// Asking the runtime for `process.execPath` / `Deno.execPath()` is the
/// shim-agnostic way to discover it, so the OS sandbox can grant the real
/// binary's directory. Returns `None` if the runtime could not be run or gave no
/// usable path (callers then simply skip the extra grant).
pub fn resolved_exec_path(
    runtime: DelegatingJsRuntimeOption,
) -> Option<std::path::PathBuf> {
    use std::process::Command;
    let output = match runtime {
        DelegatingJsRuntimeOption::Node | DelegatingJsRuntimeOption::Bun => {
            let bin = if runtime == DelegatingJsRuntimeOption::Node {
                "node"
            } else {
                "bun"
            };
            Command::new(bin)
                .args(["-e", "process.stdout.write(process.execPath)"])
                .output()
        }
        DelegatingJsRuntimeOption::Deno => Command::new("deno")
            .args([
                "eval",
                "--allow-read",
                "await Deno.stdout.write(new TextEncoder().encode(Deno.execPath()))",
            ])
            .output(),
        DelegatingJsRuntimeOption::Auto => return None,
    }
    .ok()?;

    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(path))
}
