/**
 * Resolve (and optionally build) the `omni` binary under test.
 *
 * Resolution order:
 *   1. `OMNI_TEST_BIN` env var, if set (must point at an existing file).
 *   2. `target/<triple>/release/omni` for each triple in `RUST_TARGET`
 *      (semicolon-separated), then `target/release/omni`, then
 *      `target/debug/omni`, all under the workspace root.
 *
 * The workspace root is `WORKSPACE_DIR` (the env var omni injects when running
 * tasks) when set, otherwise it is discovered by walking up to the workspace
 * `Cargo.toml`.
 *
 * This mirrors the binary-lookup convention used by the other Rust-binary
 * service tests in this repo (see `omni-remote-cache-service-tests` and
 * `bridge-service-tests`) so the suite works in CI cross-compilation lanes,
 * where the binary lands in `target/<triple>/release/`.
 */

import { existsSync } from "node:fs";
import { join } from "node:path";
import { execaSync } from "execa";
import { findRepoRoot } from "./paths";

const BIN_NAME = process.platform === "win32" ? "omni.exe" : "omni";

let cachedBin: string | undefined;

export type CargoProfile = "debug" | "release";

export interface ResolveOmniBinOptions {
    /** Explicit override; defaults to the `OMNI_TEST_BIN` env var. */
    override?: string;
    /**
     * Restrict resolution to a single profile. When omitted, a release build
     * (cross-compiled or native) is preferred over a debug build.
     */
    profile?: CargoProfile;
    /**
     * Workspace root override. Defaults to `WORKSPACE_DIR`, then the discovered
     * repo root.
     */
    root?: string;
    /**
     * Cargo target triples to search under `target/<triple>/release/`. Defaults
     * to the semicolon-separated `RUST_TARGET` env var.
     */
    targets?: string[];
}

/** Resolve the workspace root: explicit > WORKSPACE_DIR > discovered. */
function resolveRoot(override?: string): string {
    if (override) return override;
    const fromEnv = process.env.WORKSPACE_DIR?.trim();
    return fromEnv ? fromEnv : findRepoRoot();
}

/** Parse `RUST_TARGET` (or an explicit list) into clean target triples. */
function resolveTargets(override?: string[]): string[] {
    const raw = override ?? (process.env.RUST_TARGET ?? "").split(";");
    return raw.map((t) => t.trim()).filter((t) => t.length > 0);
}

function candidatePaths(
    root: string,
    profile: CargoProfile | undefined,
    targets: string[],
): string[] {
    const debug = join(root, "target", "debug", BIN_NAME);
    const release = join(root, "target", "release", BIN_NAME);
    const crossReleases = targets.map((t) =>
        join(root, "target", t, "release", BIN_NAME),
    );

    if (profile === "debug") return [debug];
    if (profile === "release") return [...crossReleases, release];
    // No explicit profile: prefer a (possibly cross-compiled) release build,
    // then a native release, then a local debug build.
    return [...crossReleases, release, debug];
}

/**
 * Return the absolute path to the omni binary, or throw a helpful error.
 */
export function resolveOmniBin(options: ResolveOmniBinOptions = {}): string {
    const override = options.override ?? process.env.OMNI_TEST_BIN;
    if (override) {
        if (!existsSync(override)) {
            throw new Error(
                `OMNI_TEST_BIN points to a missing binary: ${override}`,
            );
        }
        return override;
    }

    const canCache = !options.profile && !options.root && !options.targets;
    if (cachedBin && canCache) {
        return cachedBin;
    }

    const root = resolveRoot(options.root);
    const targets = resolveTargets(options.targets);
    const candidates = candidatePaths(root, options.profile, targets);
    for (const candidate of candidates) {
        if (existsSync(candidate)) {
            if (canCache) {
                cachedBin = candidate;
            }
            return candidate;
        }
    }

    throw new Error(
        `Could not find the omni binary. Looked in:\n  ${candidates.join("\n  ")}\n` +
            `Build it with \`cargo build -p omni\` (or \`--release\`), ` +
            `or set OMNI_TEST_BIN to an existing binary.`,
    );
}

/**
 * Ensure an omni binary exists, building it with cargo if necessary.
 *
 * Intended for use from a Vitest global setup. It first looks for any
 * already-built binary (covering CI, where the binary is produced by the
 * `omni#build` task dependency - possibly cross-compiled into
 * `target/<triple>/release/`). Only when none is found does it build one
 * locally. Honors:
 *   - `OMNI_TEST_BIN`: if set, no build happens.
 *   - `OMNI_TEST_BUILD` (`1`/`true`): force a build even if a binary exists.
 *   - `OMNI_TEST_PROFILE` (`debug` | `release`): which profile to build when a
 *     build is needed (default `debug`, the fastest to produce).
 *
 * @returns the resolved binary path.
 */
export function ensureOmniBinary(): string {
    if (process.env.OMNI_TEST_BIN) {
        return resolveOmniBin();
    }

    const forceBuild = /^(1|true)$/i.test(process.env.OMNI_TEST_BUILD ?? "");

    if (!forceBuild) {
        try {
            // Prefer any existing binary: cross-release, native release, or debug.
            return resolveOmniBin();
        } catch {
            // Nothing built yet - fall through and build one locally.
        }
    }

    const profile = (process.env.OMNI_TEST_PROFILE as CargoProfile) ?? "debug";
    const root = resolveRoot();
    const args = ["build", "-p", "omni"];
    if (profile === "release") {
        args.push("--release");
    }

    execaSync("cargo", args, { cwd: root, stdio: "inherit" });

    cachedBin = undefined;
    return resolveOmniBin({ profile });
}
