import { createRequire } from "node:module";
import type { CapabilityPolicy } from "./capability-policy";
import { NetworkPolicyError } from "./enforced-net";
import { ProcessPolicyError } from "./enforced-process";

/**
 * In-process interception of the built-in module bindings a script can reach
 * *without* going through `ctx.sys` — raw TCP sockets and direct child-process
 * spawns — so that `net` / `process` enforcement is not limited to the global
 * `fetch` and `ctx.sys.proc.spawn` surfaces.
 *
 * ## Why in-process (not a `--import` preload / Bun plugin)
 *
 * Generator scripts run in *this* bridge process and are `import()`ed lazily,
 * only after {@link installGlobalEnforcement} has run at startup. Patching the
 * builtins here therefore happens strictly *before* any untrusted script is
 * even loaded, so a script cannot capture a pre-patch reference — the same
 * guarantee a separate preload file would give, without shipping one. (A
 * separate preload only matters when the untrusted code is the process's own
 * entrypoint, which is never the case here.)
 *
 * For the exports patch to reach Bun (which snapshots a module's ESM named
 * bindings at its *first* import) the mutation must happen before that first
 * import. The bridge deliberately has **no** eager `import` of
 * `node:child_process` anywhere in its startup graph (see `enforced-process.ts`,
 * which `require`s `spawn` lazily precisely for this reason), so this patch runs
 * first and the generator script's later `import { spawn }` observes it.
 *
 * ## Reach and honest limits
 *
 * This is **defense-in-depth**, not an un-bypassable floor. It closes the
 * common, ergonomic bypasses (`import { connect } from "node:net"`,
 * `http.request(...)`, `child_process.spawn(...)`, `Deno.connect(...)`,
 * `new Deno.Command(...)`). It cannot stop a script that re-derives a binding
 * through FFI / N-API / `process.binding`, a fresh realm, or a raw syscall.
 * The un-bypassable confinement remains the runtime's own launch flags
 * (Deno `--allow-net` / `--allow-run`) and the OS sandbox (Landlock on Linux).
 *
 * Every patch is best-effort and guarded: a runtime that lacks a given builtin
 * (or forbids mutating it) simply keeps the un-patched binding rather than
 * failing the run.
 */

/** A resolved TCP target extracted from a connect call. */
export interface NetTarget {
    host: string;
    port: number;
}

/** The default host Node assumes when a `connect` call omits one. */
const DEFAULT_CONNECT_HOST = "localhost";

/**
 * Extract the TCP `{ host, port }` a `net`/`tls` connect call targets, from the
 * several overloaded argument shapes:
 *
 * * `connect(port[, host][, listener])`
 * * `connect(options[, listener])` where `options` has `port` (+ optional `host`)
 * * `connect(path[, listener])` / `{ path }` — a Unix-domain / named-pipe IPC
 *   endpoint that has no `host:port`
 *
 * Returns `null` when the target is an IPC endpoint or cannot be determined; the
 * caller then lets the connection proceed (the runtime flag / OS sandbox is the
 * floor for what this layer cannot resolve). `net` policy governs outbound TCP.
 */
export function netTargetFromConnectArgs(args: unknown[]): NetTarget | null {
    const first = args[0];

    // `net.connect`/`net.createConnection` normalize their overloads into a
    // single `[options, callback]` array before invoking
    // `Socket.prototype.connect`, so the patched prototype receives that array
    // rather than the raw call form. Unwrap it and re-extract. (A direct
    // `socket.connect(port, host)` on a Socket instance still arrives raw.)
    if (Array.isArray(first)) {
        return netTargetFromConnectArgs(first);
    }

    if (typeof first === "number") {
        const host =
            typeof args[1] === "string" ? args[1] : DEFAULT_CONNECT_HOST;
        return { host, port: first };
    }

    // A string first argument is an IPC path (Unix socket / Windows pipe).
    if (typeof first === "string") {
        return null;
    }

    if (first && typeof first === "object") {
        const options = first as Record<string, unknown>;
        // IPC endpoint — no host:port to authorize.
        if (typeof options.path === "string") {
            return null;
        }
        if (options.port !== undefined) {
            const port = Number(options.port);
            if (Number.isFinite(port)) {
                const host =
                    typeof options.host === "string"
                        ? options.host
                        : DEFAULT_CONNECT_HOST;
                return { host, port };
            }
        }
    }

    return null;
}

/**
 * Extract the program name a child-process call would execute.
 *
 * * `spawn`/`spawnSync`/`execFile`/`execFileSync`/`fork` take the program as the
 *   first argument.
 * * `exec`/`execSync` take a whole shell command line; the program is its first
 *   whitespace-delimited token (best-effort — the policy vocabulary is program
 *   names, not command lines).
 *
 * Returns `null` when no program can be determined; the caller then lets the
 * spawn proceed (the runtime `--allow-run` / OS sandbox is the floor).
 */
export function programFromArg(arg: unknown): string | null {
    if (typeof arg !== "string" || arg.length === 0) {
        return null;
    }
    return arg;
}

/** The program token of a shell command line (`"git status"` → `"git"`). */
export function programFromCommandLine(commandLine: unknown): string | null {
    if (typeof commandLine !== "string") {
        return null;
    }
    const token = commandLine.trim().split(/\s+/)[0];
    return token && token.length > 0 ? token : null;
}

/**
 * The normalized options object `ChildProcess.prototype.spawn(options)` receives,
 * from which the program can be recovered. Node/Bun/Deno all populate the same
 * shape: `file` is the resolved executable and `args` is the full argv
 * (`[file, ...]`). For a *shell* invocation (`exec(...)` or
 * `spawn(cmd, { shell: true })`) `file` is the shell itself and the real command
 * line is the trailing argv element after the `-c` / `/c` flag.
 */
interface SpawnProtoOptions {
    file?: unknown;
    args?: unknown;
    shell?: unknown;
}

/**
 * Recover the program a `ChildProcess.prototype.spawn(options)` call will
 * execute, mirroring the export-level extraction so the two layers never
 * disagree: a shell invocation resolves to the first token of its command line
 * (the same as {@link programFromCommandLine} on `exec`), a direct spawn to its
 * executable (the same as {@link programFromArg} on `spawn`/`execFile`).
 */
export function programFromSpawnOptions(options: unknown): string | null {
    if (!options || typeof options !== "object") {
        return null;
    }
    const { file, args, shell } = options as SpawnProtoOptions;
    if (shell && Array.isArray(args) && args.length > 0) {
        return programFromCommandLine(args[args.length - 1]);
    }
    return programFromArg(file);
}

/** Authorize a TCP target against the `net` policy, throwing when denied. */
function enforceNet(policy: CapabilityPolicy, target: NetTarget | null): void {
    if (target && !policy.checkNet(target.host, target.port)) {
        throw new NetworkPolicyError(target.host, target.port);
    }
}

/** Authorize a program against the `process` policy, throwing when denied. */
function enforceProgram(
    policy: CapabilityPolicy,
    program: string | null,
): void {
    if (program !== null && !policy.checkProcess(program)) {
        throw new ProcessPolicyError(program);
    }
}

/**
 * Replace an own, writable, function-valued property with a wrapper, returning
 * whether the swap succeeded. Guards against read-only / getter-only builtins
 * so a locked-down runtime never turns a patch attempt into a hard failure.
 */
function tryPatch<T extends Record<string, unknown>>(
    target: T | undefined,
    key: keyof T & string,
    wrap: (original: (...args: unknown[]) => unknown) => T[typeof key],
): boolean {
    try {
        const original = target?.[key];
        if (typeof original !== "function") {
            return false;
        }
        const descriptor = Object.getOwnPropertyDescriptor(target, key);
        if (descriptor && descriptor.writable === false && !descriptor.set) {
            return false;
        }
        (target as Record<string, unknown>)[key] = wrap(
            original as (...args: unknown[]) => unknown,
        );
        return true;
    } catch {
        return false;
    }
}

/**
 * Install best-effort, in-process enforcement of the built-in `net`/`process`
 * bindings for the given `policy`. Only the domains the policy is responsible
 * for are patched; a domain absent from the residual is left untouched (the
 * runtime already confines it precisely). Safe to call more than once and safe
 * on any runtime — every patch is guarded.
 */
export function installBuiltinModuleEnforcement(
    policy: CapabilityPolicy,
): void {
    let nodeRequire: NodeJS.Require | undefined;
    try {
        nodeRequire = createRequire(import.meta.url);
    } catch {
        nodeRequire = undefined;
    }

    if (policy.hasNet()) {
        patchNet(policy, nodeRequire);
        patchDenoNet(policy);
    }
    if (policy.hasProcess()) {
        patchChildProcess(policy, nodeRequire);
        patchDenoProcess(policy);
    }

    // Propagate the mutated CJS export objects to ESM live bindings
    // (`import { connect } from "node:net"`). Node-only; a no-op elsewhere.
    try {
        const mod = nodeRequire?.("node:module") as
            | { syncBuiltinESMExports?: () => void }
            | undefined;
        mod?.syncBuiltinESMExports?.();
    } catch {
        // best-effort
    }
}

/**
 * Patch `node:net` and `node:tls` so raw sockets are authorized. The chokepoint
 * is `net.Socket.prototype.connect`: `net.connect`/`createConnection` funnel
 * through it, and `node:http(s)` / undici's `fetch` open their sockets through
 * it too, so a single prototype patch covers them all. `tls.connect` is patched
 * as well since its connection path does not always reuse that prototype method.
 */
function patchNet(
    policy: CapabilityPolicy,
    nodeRequire: NodeJS.Require | undefined,
): void {
    if (!nodeRequire) {
        return;
    }
    try {
        const net = nodeRequire("node:net") as {
            Socket?: { prototype?: Record<string, unknown> };
        };
        tryPatch(
            net.Socket?.prototype,
            "connect",
            (original) =>
                function patchedConnect(this: unknown, ...args: unknown[]) {
                    enforceNet(policy, netTargetFromConnectArgs(args));
                    return original.apply(this, args);
                },
        );
    } catch {
        // no node:net on this runtime
    }

    try {
        const tls = nodeRequire("node:tls") as Record<string, unknown>;
        tryPatch(
            tls,
            "connect",
            (original) =>
                function patchedTlsConnect(this: unknown, ...args: unknown[]) {
                    enforceNet(policy, netTargetFromConnectArgs(args));
                    return original.apply(this, args);
                },
        );
    } catch {
        // no node:tls on this runtime
    }
}

/**
 * Patch `node:child_process` so direct-import spawns are authorized.
 *
 * Two complementary interception points, because they cover different runtimes:
 *
 * 1. **The module exports** (`spawn`, `spawnSync`, `exec`, …). The
 *    argument-first functions take the program as the first argument; the shell
 *    forms take a command line whose first token is the program. Mutating these
 *    reaches `import { spawn } from "node:child_process"` on Node (live ESM
 *    bindings, kept in sync by {@link installBuiltinModuleEnforcement}), but
 *    **not** on Bun, which snapshots ESM named bindings at first import.
 * 2. **`ChildProcess.prototype.spawn`** — the shared-prototype chokepoint the
 *    entire *async* family (`spawn`/`exec`/`execFile`/`fork`) funnels through
 *    (`new ChildProcess().spawn(options)`). Patching a shared prototype method
 *    propagates across module graphs regardless of a runtime's binding
 *    snapshotting (the same mechanism as the `net.Socket.prototype.connect`
 *    patch), so it closes the async family on Bun where the exports patch
 *    cannot. The *sync* family (`spawnSync`/`execSync`/`execFileSync`) creates
 *    no `ChildProcess` and does not route through the prototype, so it relies
 *    solely on the exports patch (1) — which reaches Bun too, because the bridge
 *    keeps its startup graph free of any eager `import` of the builtin (see the
 *    module doc and `enforced-process.ts`).
 */
function patchChildProcess(
    policy: CapabilityPolicy,
    nodeRequire: NodeJS.Require | undefined,
): void {
    if (!nodeRequire) {
        return;
    }
    let cp: Record<string, unknown>;
    try {
        cp = nodeRequire("node:child_process") as Record<string, unknown>;
    } catch {
        return;
    }

    for (const key of [
        "spawn",
        "spawnSync",
        "execFile",
        "execFileSync",
        "fork",
    ]) {
        tryPatch(
            cp,
            key,
            (original) =>
                function patchedSpawn(this: unknown, ...args: unknown[]) {
                    enforceProgram(policy, programFromArg(args[0]));
                    return original.apply(this, args);
                },
        );
    }

    for (const key of ["exec", "execSync"]) {
        tryPatch(
            cp,
            key,
            (original) =>
                function patchedExec(this: unknown, ...args: unknown[]) {
                    enforceProgram(policy, programFromCommandLine(args[0]));
                    return original.apply(this, args);
                },
        );
    }

    // The shared-prototype chokepoint: closes the async family on runtimes whose
    // ESM bindings the exports patch above cannot reach (Bun). Reads the program
    // from the normalized options the runtime hands `spawn`, matching the
    // exports-level extraction so the two agree when both fire (Node/Deno).
    const childProcess = cp.ChildProcess as
        | { prototype?: Record<string, unknown> }
        | undefined;
    tryPatch(
        childProcess?.prototype,
        "spawn",
        (original) =>
            function patchedProtoSpawn(this: unknown, ...args: unknown[]) {
                enforceProgram(policy, programFromSpawnOptions(args[0]));
                return original.apply(this, args);
            },
    );
}

/**
 * Patch Deno's native networking globals (`Deno.connect`, `Deno.connectTls`).
 * Deno's own `fetch` is intercepted by the global-`fetch` patch; these cover raw
 * TCP/TLS sockets opened through the `Deno` namespace.
 */
function patchDenoNet(policy: CapabilityPolicy): void {
    const deno = (globalThis as { Deno?: Record<string, unknown> }).Deno;
    if (!deno) {
        return;
    }
    for (const key of ["connect", "connectTls"]) {
        tryPatch(
            deno,
            key,
            (original) =>
                function patchedDenoConnect(this: unknown, ...args: unknown[]) {
                    const options = args[0] as
                        | { hostname?: unknown; port?: unknown; path?: unknown }
                        | undefined;
                    if (options && typeof options === "object") {
                        // Unix-domain transport has no host:port.
                        if (typeof options.path !== "string") {
                            const port = Number(options.port);
                            if (Number.isFinite(port)) {
                                const host =
                                    typeof options.hostname === "string"
                                        ? options.hostname
                                        : DEFAULT_CONNECT_HOST;
                                enforceNet(policy, { host, port });
                            }
                        }
                    }
                    return original.apply(this, args);
                },
        );
    }
}

/**
 * Patch Deno's child-process globals: the modern `Deno.Command` constructor and
 * the deprecated `Deno.run`.
 */
function patchDenoProcess(policy: CapabilityPolicy): void {
    const deno = (globalThis as { Deno?: Record<string, unknown> }).Deno;
    if (!deno) {
        return;
    }

    tryPatch(deno, "Command", (Original) => {
        const Wrapped = function (this: unknown, ...args: unknown[]) {
            enforceProgram(policy, programFromArg(args[0]));
            // `Deno.Command` is a class; reconstruct with `new`.
            return Reflect.construct(
                Original as unknown as new (
                    ...a: unknown[]
                ) => unknown,
                args,
            );
        };
        return Wrapped as unknown as Record<string, unknown>["Command"];
    });

    tryPatch(
        deno,
        "run",
        (original) =>
            function patchedDenoRun(this: unknown, ...args: unknown[]) {
                const options = args[0] as { cmd?: unknown } | undefined;
                const cmd = options?.cmd;
                if (Array.isArray(cmd)) {
                    enforceProgram(policy, programFromArg(cmd[0]));
                }
                return original.apply(this, args);
            },
    );
}
