import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { delimiter, join } from "node:path";
import type { SpawnOptions, SpawnResult } from "@omni-oss/gen-sdk-core";
import type { CapabilityPolicy } from "./capability-policy";
import { isEnvInjectionVector, isPathKey } from "./enforced-env";

/**
 * Lazily resolve `node:child_process`'s `spawn` at call time via `require`
 * rather than a top-level `import`.
 *
 * This is load-bearing for Bun enforcement, not a style choice: Bun snapshots a
 * module's ESM named bindings the **first** time it is ESM-imported, and a
 * later mutation of the exports (which is how {@link installBuiltinModuleEnforcement}
 * patches `spawn`/`spawnSync`/…) no longer reaches those frozen bindings. A
 * top-level `import { spawn } from "node:child_process"` here would be that
 * first ESM import — executed as the bundle loads, *before* enforcement runs —
 * freezing the binding and defeating the patch for every generator script. A
 * `require` reads the (patched) exports object without taking an ESM snapshot,
 * so it must stay lazy. Do not convert this back to a static import.
 */
let cachedNodeSpawn: typeof import("node:child_process").spawn | undefined;
function getNodeSpawn(): typeof import("node:child_process").spawn {
    if (!cachedNodeSpawn) {
        cachedNodeSpawn = (
            createRequire(import.meta.url)(
                "node:child_process",
            ) as typeof import("node:child_process")
        ).spawn;
    }
    return cachedNodeSpawn;
}

/** Thrown when a spawn is refused by the `process` capability policy. */
export class ProcessPolicyError extends Error {
    constructor(program: string) {
        super(
            `capability policy denied spawning process "${program}" ` +
                `(not permitted by this generator's \`process\` policy)`,
        );
        this.name = "ProcessPolicyError";
    }
}

/**
 * The signature of {@link @omni-oss/gen-sdk-core!EnforcedProcess.spawn}.
 */
export type EnforcedSpawn = (
    program: string,
    options?: SpawnOptions,
) => Promise<SpawnResult>;

/**
 * The working directory to launch a child under: the caller's explicit `cwd`,
 * otherwise the script's virtualized current directory.
 *
 * The virtual cwd (typically the generator's output directory) may not exist on
 * the real filesystem yet — the generator's own writes are staged in a
 * transaction and only materialize on commit. Spawning with a `cwd` that does
 * not exist fails the underlying `posix_spawn`/`chdir` with a misleading
 * `ENOENT: … 'program'`, so a non-existent directory is dropped and the child
 * inherits the (real, existing) parent working directory instead.
 */
function resolveCwd(
    explicit: string | undefined,
    fallback: string,
): string | undefined {
    const cwd = explicit ?? fallback;
    return cwd && existsSync(cwd) ? cwd : undefined;
}

/**
 * Environment variables a confined child inherits from the host, by name.
 *
 * A confined spawn is given an *explicit*, minimal environment rather than the
 * host's full one: this avoids leaking ambient secrets into the child and, on
 * Deno, sidesteps the `--allow-env` error its `node:child_process` layer would
 * otherwise raise by enumerating the whole environment. These are the
 * non-sensitive vars ordinary tools need to run (locating binaries, locale,
 * temp dir). Kept in sync with the Deno backend's `--allow-env` grant in
 * `crates/omni_capability_enforcement/src/deno.rs`.
 */
const INHERITED_ENV_KEYS = [
    "PATH",
    "HOME",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "TERM",
    "TZ",
    "NODE_V8_COVERAGE",
] as const;

/**
 * The explicit environment handed to a confined child: the allow-listed host
 * vars ({@link INHERITED_ENV_KEYS}) that are set, plus any per-call `overrides`.
 * Only allow-listed keys are read from `process.env`, so on Deno no variable
 * outside the granted set is ever accessed.
 *
 * The `overrides` are attacker-influenced (a generator script controls them), so
 * they are **not** merged verbatim: every code-injection vector (mirroring the
 * Rust `ENV_INJECTION_DENYLIST`) and any attempt to override `PATH` are dropped.
 * Dropping `PATH` keeps the trusted, inherited value in place so a policy that
 * authorized a bare program name (`git`) cannot be redirected to an attacker
 * binary via `PATH=/tmp/evil:...`. Exported for testing.
 */
export function confinedEnv(
    overrides?: Record<string, string>,
): Record<string, string> {
    const env: Record<string, string> = {};
    for (const key of INHERITED_ENV_KEYS) {
        // Read defensively: on Deno, reading a variable the launch flags did not
        // grant (`--allow-env`) throws rather than returning `undefined`. A key
        // can legitimately be un-granted when the `env` policy explicitly denies
        // it (the Deno backend subtracts denied keys from the bootstrap grant),
        // in which case the child must simply not inherit it — never crash the
        // spawn. Keys that are granted still read normally.
        let value: string | undefined;
        try {
            value = process.env[key];
        } catch {
            continue;
        }
        if (value !== undefined) {
            env[key] = value;
        }
    }
    for (const [key, value] of Object.entries(overrides ?? {})) {
        // A caller override can never re-introduce a linker/code-injection
        // vector nor take over `PATH` (the trusted inherited value wins).
        if (isEnvInjectionVector(key) || isPathKey(key)) {
            continue;
        }
        env[key] = value;
    }
    return env;
}

/**
 * Resolve a bare program name to an absolute path using the *trusted* `PATH`
 * (never a caller override), so a confined spawn runs the binary the policy
 * authorized rather than one an attacker planted earlier on a caller-controlled
 * path. A name that already contains a path separator, or one not found on the
 * trusted path, is returned unchanged — the child then resolves it against the
 * same trusted `PATH` we pin into its environment, so it is never redirectable
 * either way. Skipped on Windows, where `PATHEXT` resolution differs and `PATH`
 * is pinned instead. Exported for testing.
 */
export function resolveProgramOnTrustedPath(
    program: string,
    trustedPath: string | undefined,
): string {
    if (process.platform === "win32") {
        return program;
    }
    if (program.length === 0 || program.includes("/") || !trustedPath) {
        return program;
    }
    for (const dir of trustedPath.split(delimiter)) {
        if (!dir) {
            continue;
        }
        const candidate = join(dir, program);
        try {
            if (existsSync(candidate)) {
                return candidate;
            }
        } catch {
            // Unreadable directory entry — keep scanning.
        }
    }
    return program;
}

/**
 * Build a capability-gated `spawn`. Every call is authorized against the
 * `process` policy before the child is launched; when the policy does not
 * enforce `process` (the runtime confines it at launch), the check is skipped
 * and the spawn proceeds (the runtime remains the floor).
 *
 * `defaultCwd` supplies the working directory when a call omits one — typically
 * the script's virtualized current directory.
 */
export function createEnforcedSpawn(
    policy: CapabilityPolicy,
    defaultCwd: () => string,
): EnforcedSpawn {
    return (program, options) =>
        new Promise<SpawnResult>((resolve, reject) => {
            if (policy.hasProcess() && !policy.checkProcess(program)) {
                reject(new ProcessPolicyError(program));
                return;
            }

            // Build the confined environment first (its `PATH` is the trusted,
            // inherited value — never a caller override), then resolve the
            // program against that same trusted `PATH`. The policy check above
            // ran on the program name the caller wrote, so authorization
            // semantics are unchanged; only the *binary actually launched* is
            // pinned, closing the `PATH`-hijack path.
            const env = confinedEnv(options?.env);
            const resolved = resolveProgramOnTrustedPath(program, env.PATH);

            const child = getNodeSpawn()(resolved, [...(options?.args ?? [])], {
                cwd: resolveCwd(options?.cwd, defaultCwd()),
                // A confined child gets an explicit, minimal environment
                // (see `confinedEnv`) rather than inheriting the host's.
                env,
                // Capture output; never inherit a TTY into a confined
                // script.
                stdio: ["ignore", "pipe", "pipe"],
            });

            let stdout = "";
            let stderr = "";
            child.stdout?.on("data", (chunk) => {
                stdout += chunk.toString();
            });
            child.stderr?.on("data", (chunk) => {
                stderr += chunk.toString();
            });

            child.on("error", reject);
            child.on("close", (code) => {
                resolve({ code, stdout, stderr });
            });
        });
}
