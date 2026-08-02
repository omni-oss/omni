/**
 * Environment scrubbing shared by every confined-spawn path in the shim
 * (`enforced-process.ts`'s `ctx.sys.proc.spawn`, and the direct
 * `node:child_process` patches in `enforced-builtins.ts`).
 *
 * A confined child — and every grandchild it spawns — must never be launchable
 * with a linker/loader or interpreter "code-injection" variable in its
 * environment, nor with a caller-controlled `PATH` that could redirect an
 * allow-listed program name (`git`) to an attacker-planted binary
 * (`/tmp/evil/git`). Both would turn a spawn the `process` policy *authorized*
 * into arbitrary code execution, defeating the sandbox.
 *
 * This mirrors the Rust runner's `ENV_INJECTION_DENYLIST`
 * (`crates/bridge_rpc_runner/src/runner.rs`), which scrubs the identical set
 * from the runtime's own launch; we extend the same rule to the in-process
 * `spawn` surface, which the Rust scrub does not reach.
 */

/**
 * Linker/loader and code-injection variables that must be dropped from a
 * confined child's environment regardless of what the caller supplied. Kept in
 * lock-step with the Rust `ENV_INJECTION_DENYLIST`; matched case-insensitively
 * so a case-insensitive OS cannot smuggle one past the check (`ld_preload`).
 */
export const ENV_INJECTION_DENYLIST = [
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FALLBACK_FRAMEWORK_PATH",
    "NODE_OPTIONS",
] as const;

const DENY_SET: ReadonlySet<string> = new Set(
    ENV_INJECTION_DENYLIST.map((key) => key.toLowerCase()),
);

/**
 * Whether `key` names a code-injection vector that must never reach a confined
 * child (see {@link ENV_INJECTION_DENYLIST}). Case-insensitive.
 */
export function isEnvInjectionVector(key: string): boolean {
    return DENY_SET.has(key.toLowerCase());
}

/**
 * Whether `key` is the executable search path (`PATH`, or `Path` on Windows).
 * The trusted parent value is always pinned into a confined child instead of a
 * caller-supplied one, so an authorized program name cannot be redirected.
 */
export function isPathKey(key: string): boolean {
    return key.toLowerCase() === "path";
}

/**
 * Scrub a caller-supplied environment for a confined spawn: drop every
 * code-injection vector and every `PATH` entry, then (when `trustedPath` is
 * given) pin `PATH` to that trusted value. The result is safe to hand to a
 * child process launcher.
 *
 * `PATH` is dropped-then-pinned rather than merged so a caller cannot prepend a
 * writable directory (`PATH=/tmp/evil:$PATH`) to hijack name resolution; the
 * child always resolves bare program names against the trusted path.
 */
export function scrubChildEnv(
    env: Record<string, string | undefined>,
    trustedPath?: string,
): Record<string, string> {
    const out: Record<string, string> = {};
    for (const [key, value] of Object.entries(env)) {
        if (value === undefined) {
            continue;
        }
        if (isEnvInjectionVector(key) || isPathKey(key)) {
            continue;
        }
        out[key] = value;
    }
    if (trustedPath !== undefined) {
        out.PATH = trustedPath;
    }
    return out;
}
