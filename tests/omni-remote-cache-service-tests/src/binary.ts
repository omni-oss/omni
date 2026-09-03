/**
 * Resolve the Rust service binary under test (defaults to
 * `omni_remote_cache_service`).
 *
 * Resolution order (deduped, each candidate probed once):
 *   1. `<BIN>_TEST_BIN` env override, if set (must point at an existing file).
 *   2. `target/debug/<bin>` (native debug).
 *   3. `target/release/<bin>` when it matches the host target.
 *   4. `target/<triple>/release/<bin>` for each triple in `RUST_TARGET`
 *      (semicolon-separated).
 *   5. `target/release/<bin>` (native release).
 *
 * The workspace root comes from `WORKSPACE_DIR` (injected by omni when running
 * tasks). This mirrors the binary-lookup convention in
 * `omni-tests/src/harness/binary.ts` while keeping the host-preference and
 * configurable binary name specific to the service test suites.
 */

import fsSync from "node:fs";
import os from "node:os";
import path from "node:path";

export type CargoProfile = "debug" | "release";

const EXT = os.platform() === "win32" ? ".exe" : "";

export interface ResolveServiceBinOptions {
    /** Base binary name without extension (e.g. `omni_remote_cache_service`). */
    binName: string;
    /**
     * Explicit override; defaults to the `<BIN_NAME_UPPER>_TEST_BIN` env var
     * (e.g. `OMNI_REMOTE_CACHE_SERVICE_TEST_BIN`).
     */
    override?: string;
    /** Restrict resolution to a single profile. */
    profile?: CargoProfile;
    /** Workspace root override. Defaults to `WORKSPACE_DIR`. */
    root?: string;
    /**
     * Cargo target triples to search under `target/<triple>/release/`. Defaults
     * to the semicolon-separated `RUST_TARGET` env var.
     */
    targets?: string[];
    /**
     * Detected host target triple. When it matches one of `targets`, the native
     * `target/release/<bin>` is preferred ahead of cross-compiled releases, but
     * still after a local debug build. Callers typically pass the result of
     * `getHost()` (or `""` when it can't be determined).
     */
    host?: string;
}

export interface ResolvedServiceBin {
    /** Absolute path to the resolved binary, or `undefined` if none exists. */
    path?: string | undefined;
    /** All probed candidate paths, in order (deduped). */
    candidates: string[];
    /** Human-readable summary useful for error messages / debugging. */
    compileInfo: string;
}

/** Derive the `<BIN>_TEST_BIN` env var name from a binary name. */
function overrideEnvName(binName: string): string {
    return `${binName.replace(/[^a-z0-9]+/gi, "_").toUpperCase()}_TEST_BIN`;
}

/** Resolve the workspace root: explicit > WORKSPACE_DIR. */
function resolveRoot(override?: string): string {
    const root = (override ?? process.env.WORKSPACE_DIR ?? "").trim();
    if (!root) {
        throw new Error(
            "Could not resolve workspace root: WORKSPACE_DIR is not set " +
                "(and no explicit root was provided).",
        );
    }
    return root;
}

/** Parse `RUST_TARGET` (or an explicit list) into clean target triples. */
function resolveTargets(override?: string[]): string[] {
    const raw = override ?? (process.env.RUST_TARGET ?? "").split(";");
    return raw.map((t) => t.trim()).filter((t) => t.length > 0);
}

/** Build the ordered, deduped list of candidate binary paths. */
function candidatePaths(
    root: string,
    binName: string,
    profile: CargoProfile | undefined,
    targets: string[],
    host: string,
): string[] {
    const bin = `${binName}${EXT}`;
    const release = path.join(root, "target", "release", bin);
    const debug = path.join(root, "target", "debug", bin);
    const crossReleases = targets.map((t) =>
        path.join(root, "target", t, "release", bin),
    );

    let ordered: string[];
    if (profile === "debug") {
        ordered = [debug];
    } else if (profile === "release") {
        ordered = [...crossReleases, release];
    } else {
        // Prefer a local debug build, then a host-matching native release,
        // then cross-compiled releases, then a native release.
        const preferNative = host !== "" && targets.includes(host);
        ordered = [
            debug,
            ...(preferNative ? [release] : []),
            ...crossReleases,
            release,
        ];
    }

    return ordered.filter((p, i, arr) => arr.indexOf(p) === i);
}

/**
 * Resolve the service binary. Never throws for a missing binary: inspect
 * `result.path` (undefined when nothing was found) and use `result.candidates`
 * / `result.compileInfo` to build an error message. Throws only for genuinely
 * broken configuration (e.g. an override pointing at a missing file, or an
 * unresolvable workspace root).
 */
export function resolveServiceBin(
    options: ResolveServiceBinOptions,
): ResolvedServiceBin {
    const { binName, host = "" } = options;

    const target = options.targets?.join(";") ?? process.env.RUST_TARGET ?? "";
    const compileInfoBase = `Host: ${host}\nTarget: ${target}`;

    const override = options.override ?? process.env[overrideEnvName(binName)];
    if (override) {
        if (!fsSync.existsSync(override)) {
            const envName = overrideEnvName(binName);
            throw new Error(
                `${envName} points to a missing binary: ${override}`,
            );
        }
        return {
            path: override,
            candidates: [override],
            compileInfo: `${compileInfoBase}\nPath: ${override}`,
        };
    }

    const root = resolveRoot(options.root);
    const targets = resolveTargets(options.targets);
    const candidates = candidatePaths(
        root,
        binName,
        options.profile,
        targets,
        host,
    );

    const resolved = candidates.find((c) => fsSync.existsSync(c));
    const compileInfo = resolved
        ? `${compileInfoBase}\nPath: ${resolved}`
        : compileInfoBase;

    return { path: resolved, candidates, compileInfo };
}

/**
 * Convenience wrapper that throws a helpful error when no binary is found.
 */
export function resolveServiceBinOrThrow(options: ResolveServiceBinOptions): {
    path: string;
    compileInfo: string;
} {
    const {
        path: resolved,
        candidates,
        compileInfo,
    } = resolveServiceBin(options);

    if (!resolved) {
        throw new Error(
            `Could not find ${options.binName}${EXT} in:\n${candidates.join(
                "\n",
            )}\n${compileInfo}`,
        );
    }

    return { path: resolved, compileInfo };
}
