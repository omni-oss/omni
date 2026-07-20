import type {
    EnforcedProcess,
    EnforcedSystem,
    FetchFn,
} from "@omni-oss/gen-sdk-core";
import type { System } from "@omni-oss/system-interface";
import { CapabilityPolicy } from "./capability-policy";
import { installBuiltinModuleEnforcement } from "./enforced-builtins";
import { createEnforcedFetch } from "./enforced-net";
import { createEnforcedSpawn } from "./enforced-process";

export { CapabilityPolicy } from "./capability-policy";
export { installBuiltinModuleEnforcement } from "./enforced-builtins";
export { createEnforcedFetch, NetworkPolicyError } from "./enforced-net";
export { createEnforcedSpawn, ProcessPolicyError } from "./enforced-process";

/**
 * The original, un-enforced `fetch`, captured at module load *before* any global
 * patch is installed. All enforcement wraps this so wrapping is never recursive,
 * regardless of whether the global has been replaced.
 */
const ORIGINAL_FETCH: FetchFn =
    typeof globalThis.fetch === "function"
        ? globalThis.fetch.bind(globalThis)
        : (globalThis.fetch as FetchFn);

/**
 * The process-wide policy in effect for this bridge process. Set once at startup
 * from the `--enforce` residual; defaults to an empty (passthrough) policy so
 * unit tests and un-flagged runs behave as before.
 */
let currentPolicy: CapabilityPolicy = CapabilityPolicy.empty();

/** The capability policy currently in effect. */
export function activePolicy(): CapabilityPolicy {
    return currentPolicy;
}

/**
 * Install process-wide capability enforcement from the residual policy.
 *
 * Called once by the bridge-service entrypoint before the RPC starts. Today it:
 *
 * * records the policy for {@link buildEnforcedSystem},
 * * patches the **global `fetch`** when the policy enforces `net`, so even a
 *   script that reaches for the ambient `fetch` (rather than
 *   `ctx.sys.net.http.fetch`) is still checked, and
 * * patches the **built-in module bindings** (`node:net`/`node:tls` sockets,
 *   `node:child_process` spawns, and Deno's `Deno.connect`/`Deno.Command`
 *   globals) via {@link installBuiltinModuleEnforcement}, so a script that
 *   imports those directly — bypassing both the global `fetch` and
 *   `ctx.sys.proc.spawn` — is still authorized.
 *
 * The builtin patch is best-effort defense-in-depth (a script can still reach
 * a raw socket through FFI / N-API / a fresh realm); the un-bypassable floor
 * remains the runtime launch flags and the OS sandbox. It runs before any
 * generator script is `import()`ed, so a script cannot capture a pre-patch
 * reference.
 */
export function installGlobalEnforcement(policy: CapabilityPolicy): void {
    currentPolicy = policy;
    if (policy.hasNet()) {
        const enforced = createEnforcedFetch(ORIGINAL_FETCH, policy);
        // Preserve any runtime-specific statics on `fetch` (e.g. Bun's
        // `fetch.preconnect`) so replacing the global does not drop them.
        const original = globalThis.fetch as { preconnect?: unknown };
        const patched = Object.assign(enforced, {
            preconnect: original.preconnect,
        });
        globalThis.fetch = patched as unknown as typeof globalThis.fetch;
    }
    // Patch the direct-import builtin bindings for whichever of net/process the
    // policy is responsible for (a no-op for domains it is not).
    installBuiltinModuleEnforcement(policy);
}

/**
 * Produce an {@link EnforcedSystem} view over `base`, adding the capability-gated
 * `net`/`proc.spawn` surface. Uses {@link activePolicy} by default so it reflects
 * whatever was installed at startup; a `policy` may be passed for tests.
 */
export function buildEnforcedSystem(
    base: System,
    policy: CapabilityPolicy = currentPolicy,
): EnforcedSystem {
    const proc: EnforcedProcess = {
        currentDir: () => base.proc.currentDir(),
        setCurrentDir: (dir: string) => base.proc.setCurrentDir(dir),
        args: () => base.proc.args(),
        env: () => base.proc.env(),
        spawn: createEnforcedSpawn(policy, () => base.proc.currentDir()),
    };

    return {
        fs: base.fs,
        proc,
        net: {
            http: {
                fetch: createEnforcedFetch(ORIGINAL_FETCH, policy),
            },
        },
    };
}
