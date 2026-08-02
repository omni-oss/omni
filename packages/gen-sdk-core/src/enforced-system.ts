import type { Process, System } from "@omni-oss/system-interface";

/**
 * A capability-enforcing view of the host system handed to generator scripts.
 *
 * It extends the base {@link System} with the domains whose I/O happens *inside*
 * the JS runtime and therefore cannot be brokered over RPC back to omni:
 * outbound network ({@link EnforcedNet}) and child-process spawning
 * ({@link EnforcedProcess}). Scripts should use these instead of the ambient
 * globals (`fetch`, `node:child_process`) so their access is checked against the
 * generator's capability policy.
 *
 * The enforcement is fed a residual policy from the spawning host: only the
 * rules the runtime's own launch flags could not confine precisely are checked
 * here (see the `--enforce` bridge-service flag). When the runtime already
 * confines a domain, these wrappers are transparent passthroughs.
 */
export interface EnforcedSystem extends System {
    net: EnforcedNet;
    proc: EnforcedProcess;
}

/** The network capability surface. Grouped by protocol family for growth. */
export interface EnforcedNet {
    http: EnforcedHttp;
}

/** HTTP(S) access. `fetch` mirrors the global `fetch` but is capability-gated. */
export interface EnforcedHttp {
    /**
     * A capability-enforcing `fetch`. Behaves like the global `fetch`, except a
     * request whose `host:port` is not permitted by the generator's `net`
     * policy rejects with an error before any connection is attempted.
     */
    fetch: FetchFn;
}

/**
 * The portable call signature of `fetch`. Deliberately narrower than
 * `typeof globalThis.fetch` (which carries runtime-specific statics such as
 * Bun's `preconnect`) so a wrapper is straightforward to type across runtimes.
 */
export type FetchFn = (
    input: RequestInfo | URL,
    init?: RequestInit,
) => Promise<Response>;

/** The result of a completed {@link EnforcedProcess.spawn}. */
export interface SpawnResult {
    /** Exit code, or `null` when the child was terminated by a signal. */
    code: number | null;
    /** Captured standard output, decoded as UTF-8. */
    stdout: string;
    /** Captured standard error, decoded as UTF-8. */
    stderr: string;
}

/** Options for {@link EnforcedProcess.spawn}. */
export interface SpawnOptions {
    /** Arguments passed to the program. */
    args?: readonly string[];
    /**
     * Working directory. Defaults to the script's current directory
     * ({@link Process.currentDir}).
     */
    cwd?: string;
    /** Extra environment variables merged over the inherited environment. */
    env?: Readonly<Record<string, string>>;
}

/**
 * The base {@link Process} plus a capability-gated {@link spawn}.
 */
export interface EnforcedProcess extends Process {
    /**
     * Spawn a child process, capturing its output. Rejects before spawning when
     * the generator's `process` policy does not permit `program`.
     */
    spawn(program: string, options?: SpawnOptions): Promise<SpawnResult>;
}
