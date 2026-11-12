/**
 * Spawn the `omni` binary and capture its result for assertions.
 */

import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

import { execa, type Options } from "execa";

import { resolveOmniBin } from "./binary";
import { normalize } from "./normalize";

export interface RunOmniOptions {
    /** Working directory to run in (usually a workspace root). */
    cwd?: string;
    /**
     * Environment variables to set. By default these are merged on top of the
     * parent process env; set {@link RunOmniOptions.cleanEnv} to start empty.
     */
    env?: Record<string, string | undefined>;
    /**
     * When true, do not inherit the parent process environment. You'll likely
     * need to provide `PATH` (and similar) yourself for tasks to spawn shells.
     */
    cleanEnv?: boolean;
    /** String piped to the process's stdin. */
    input?: string;
    /** Kill the process after this many milliseconds. Default 30_000. */
    timeout?: number;
}

export interface OmniResult {
    /** Process exit code (0 on success). */
    exitCode: number;
    /** Raw stdout. */
    stdout: string;
    /** Raw stderr. */
    stderr: string;
    /** `normalize(stdout)` - line endings + trailing newlines stripped. */
    out: string;
    /** `normalize(stderr)`. */
    err: string;
    /** True when the process exited non-zero, timed out, or was killed. */
    failed: boolean;
    /** True when the process was killed by the configured timeout. */
    timedOut: boolean;
    /** The full command line that was executed (for diagnostics). */
    command: string;
}

const DEFAULT_TIMEOUT_MS = 30_000;

const WORKSPACE_FILES = ["yaml", "yml", "json", "toml"].map(
    (ext) => `workspace.omni.${ext}`,
);

/**
 * Keep remote sources out of the developer's real per-user store: unless the
 * caller sets `OMNI_SOURCES_DIR`, point the store at the workspace's own
 * `.omni/sources/store`. Tests that exercise the global default set it
 * themselves (or pass `undefined` to unset it).
 */
function isolatedSourcesDir(
    cwd: string | undefined,
    env: Record<string, string | undefined> | undefined,
): string | undefined {
    if (env && "OMNI_SOURCES_DIR" in env) return undefined;
    if (process.env.OMNI_SOURCES_DIR) return undefined;
    let dir = resolve(cwd ?? process.cwd());
    for (;;) {
        if (WORKSPACE_FILES.some((name) => existsSync(join(dir, name)))) {
            return join(dir, ".omni", "sources", "store");
        }
        const parent = dirname(dir);
        if (parent === dir) return undefined;
        dir = parent;
    }
}

/**
 * Run `omni <...args>` and resolve with its captured output and exit code.
 *
 * The promise never rejects on a non-zero exit code - inspect
 * {@link OmniResult.exitCode} / {@link OmniResult.failed} instead. It will
 * still reject if the binary cannot be found or spawned at all.
 *
 * @example
 * const result = await runOmni(["--version"]);
 * expect(result).toSucceed();
 * expect(result.out).toMatch(/^\d+\.\d+\.\d+$/);
 */
export async function runOmni(
    args: string[],
    options: RunOmniOptions = {},
): Promise<OmniResult> {
    const bin = resolveOmniBin();

    // Build options conditionally: with exactOptionalPropertyTypes enabled we
    // must not pass `undefined` for optional execa options.
    const sourcesDir = isolatedSourcesDir(options.cwd, options.env);
    const env =
        sourcesDir !== undefined
            ? { ...options.env, OMNI_SOURCES_DIR: sourcesDir }
            : options.env;
    const execaOptions: Options = {
        extendEnv: !options.cleanEnv,
        timeout: options.timeout ?? DEFAULT_TIMEOUT_MS,
        reject: false,
        stripFinalNewline: false,
        all: false,
        ...(options.cwd !== undefined ? { cwd: options.cwd } : {}),
        ...(env !== undefined ? { env } : {}),
        ...(options.input !== undefined ? { input: options.input } : {}),
    };

    const result = await execa(bin, args, execaOptions);

    const stdout = typeof result.stdout === "string" ? result.stdout : "";
    const stderr = typeof result.stderr === "string" ? result.stderr : "";

    return {
        exitCode: result.exitCode ?? (result.failed ? 1 : 0),
        stdout,
        stderr,
        out: normalize(stdout),
        err: normalize(stderr),
        failed: result.failed,
        timedOut: result.timedOut ?? false,
        command: result.command,
    };
}
