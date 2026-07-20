import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import type { SpawnOptions, SpawnResult } from "@omni-oss/gen-sdk-core";
import type { CapabilityPolicy } from "./capability-policy";

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
 */
function confinedEnv(
    overrides?: Record<string, string>,
): Record<string, string> {
    const env: Record<string, string> = {};
    for (const key of INHERITED_ENV_KEYS) {
        const value = process.env[key];
        if (value !== undefined) {
            env[key] = value;
        }
    }
    return { ...env, ...(overrides ?? {}) };
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

            const child = getNodeSpawn()(program, [...(options?.args ?? [])], {
                cwd: resolveCwd(options?.cwd, defaultCwd()),
                // A confined child gets an explicit, minimal environment
                // (see `confinedEnv`) rather than inheriting the host's.
                env: confinedEnv(options?.env),
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
