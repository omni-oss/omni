import type {
    EnforcedProcess,
    EnforcedSystem,
    FetchFn,
} from "@omni-oss/gen-sdk-core";
import type { System } from "@omni-oss/system-interface";
import { CapabilityPolicy } from "./capability-policy";
import {
    defineEnforcedGlobal,
    installBuiltinModuleEnforcement,
} from "./enforced-builtins";
import {
    createEnforcedFetch,
    NetworkPolicyError,
    netTargetFromUrl,
} from "./enforced-net";
import { createEnforcedSpawn } from "./enforced-process";

export { CapabilityPolicy } from "./capability-policy";
export {
    installBuiltinModuleEnforcement,
    RealmPolicyError,
} from "./enforced-builtins";
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
 *   `node:child_process` spawns, `node:dgram` UDP, `node:http2`, the global
 *   `WebSocket`, and Deno's `Deno.connect`/`Deno.Command` globals) and **gates
 *   the fresh-realm escapes** (`worker_threads.Worker`, `node:vm` new contexts)
 *   via {@link installBuiltinModuleEnforcement}, so a script that imports those
 *   directly — bypassing both the global `fetch` and `ctx.sys.proc.spawn` — is
 *   still authorized.
 *
 * The builtin patch is best-effort defense-in-depth (a script can still reach
 * a raw socket through FFI / N-API); the un-bypassable floor remains the
 * runtime launch flags and the OS sandbox. It runs before any generator script
 * is `import()`ed, so a script cannot capture a pre-patch reference.
 */
export function installGlobalEnforcement(policy: CapabilityPolicy): void {
    currentPolicy = policy;
    if (policy.hasNet()) {
        const enforced = createEnforcedFetch(ORIGINAL_FETCH, policy);
        // Preserve any runtime-specific statics on `fetch` (e.g. Bun's
        // `fetch.preconnect`) so replacing the global does not drop them — but
        // `preconnect` opens a socket to a host *ahead* of the request, so it is
        // a distinct egress path that bypasses the wrapped `fetch`. Wrap it to
        // authorize the same target rather than copy the raw one verbatim.
        const original = globalThis.fetch as {
            preconnect?: (...args: unknown[]) => unknown;
        };
        const patched = enforced as FetchFn & {
            preconnect?: (...args: unknown[]) => unknown;
        };
        if (typeof original.preconnect === "function") {
            patched.preconnect = createEnforcedPreconnect(
                original.preconnect.bind(globalThis.fetch),
                policy,
            );
        }
        // Lock the global against reassignment (`globalThis.fetch = raw`) and
        // against `delete`/redefinition so untrusted code cannot swap the
        // wrapper out; the only way to recover a raw `fetch` is a fresh realm,
        // itself gated by {@link installBuiltinModuleEnforcement}. The lock is
        // non-writable **and** non-configurable — a repeat install stays
        // idempotent via the enforced-globals marker rather than by leaving the
        // slot redefinable. Fall back to a plain assignment only on runtimes
        // that forbid the redefinition outright.
        const target = globalThis as unknown as Record<string, unknown>;
        if (!defineEnforcedGlobal(target, "fetch", patched)) {
            globalThis.fetch = patched as unknown as typeof globalThis.fetch;
        }
    }
    // Patch the direct-import builtin bindings for whichever of net/process the
    // policy is responsible for (a no-op for domains it is not).
    installBuiltinModuleEnforcement(policy);
}

/**
 * Wrap a runtime's `fetch.preconnect(url)` so the eager connection it opens is
 * authorized against the same `net` policy as the request that follows. An
 * un-parseable argument is passed through to the raw implementation (which will
 * reject it) rather than guessed at. Exported for testing.
 */
export function createEnforcedPreconnect(
    rawPreconnect: (...args: unknown[]) => unknown,
    policy: CapabilityPolicy,
): (...args: unknown[]) => unknown {
    return (...args: unknown[]) => {
        const target = netTargetFromUrl(args[0]);
        if (target && !policy.checkNet(target.host, target.port)) {
            throw new NetworkPolicyError(target.host, target.port);
        }
        return rawPreconnect(...args);
    };
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
