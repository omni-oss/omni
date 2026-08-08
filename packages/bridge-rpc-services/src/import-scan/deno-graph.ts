// Strategy B (Deno) — the runtime's own static graph.
//
// `deno info --json <entry…>` produces Deno's fully-resolved module graph
// WITHOUT executing any of the scanned code. Each on-disk module carries a
// `local` path (the exact file closure to grant read); each npm dependency is
// reported under `npmPackages[key].localPath` (grant that package root as a
// bounded boundary, like the strategy-C `node_modules` package roots).
//
// This is preferred over strategy C on Deno because Deno's resolver is not
// exposed as an importer-parametrized `resolve`, and `deno info` already gives
// the exact closure the runtime will load.

import type { ClosureResult } from "./closure";
import path from "node:path";
import { pathToFileURL } from "node:url";

/**
 * Normalize a scan entry to a `file:` URL when it is a filesystem path.
 *
 * `deno info --json` only populates a module's `local` field (the on-disk path
 * we grant) when the entry is addressed by URL. Passed a bare absolute path
 * (e.g. `C:\\…\\gen.mjs`), Deno reports the root module's specifier as that raw
 * path with `local` left undefined, so it would be silently dropped from the
 * closure. Already-URL entries (`file:`, `npm:`, `jsr:`, `http(s):`, `data:`)
 * are passed through unchanged.
 */
export function toEntryUrl(entry: string): string {
    // A scheme followed by `//` (file://, http://), or a bare scheme like
    // `npm:`/`jsr:`/`node:`/`data:`. A Windows drive path (`C:\\…`) is NOT a URL
    // despite matching `x:`.
    const isWindowsDrivePath = /^[a-zA-Z]:[\\/]/.test(entry);
    const isUrl =
        !isWindowsDrivePath &&
        /^[a-z][a-z0-9+\-.]*:/i.test(entry);
    if (isUrl) {
        return entry;
    }
    return pathToFileURL(path.resolve(entry)).href;
}

interface DenoInfoModule {
    local?: string | null;
}

interface DenoInfoNpmPackage {
    localPath?: string | null;
}

interface DenoInfo {
    modules?: DenoInfoModule[];
    npmPackages?: Record<string, DenoInfoNpmPackage>;
}

interface DenoNamespace {
    Command: new (
        command: string,
        options: { args: string[]; stdout: "piped"; stderr: "piped" },
    ) => {
        output(): Promise<{
            success: boolean;
            stdout: Uint8Array;
            stderr: Uint8Array;
        }>;
    };
    execPath(): string;
}

function denoGlobal(): DenoNamespace | undefined {
    return (globalThis as { Deno?: DenoNamespace }).Deno;
}

/**
 * Compute the closure for `entries` on Deno by parsing `deno info --json`.
 * The scan runs unconfined and executes no scanned code (`deno info` only
 * type-checks/resolves the graph).
 */
export async function computeDenoClosure(
    entries: string[],
): Promise<ClosureResult> {
    const deno = denoGlobal();
    if (!deno) {
        throw new Error("the Deno strategy requires the Deno runtime");
    }

    const diagnostics: string[] = [];
    const cmd = new deno.Command(deno.execPath(), {
        args: ["info", "--json", ...entries.map(toEntryUrl)],
        stdout: "piped",
        stderr: "piped",
    });
    const { success, stdout, stderr } = await cmd.output();
    if (!success) {
        throw new Error(
            `deno info failed: ${new TextDecoder().decode(stderr)}`,
        );
    }

    const info = JSON.parse(new TextDecoder().decode(stdout)) as DenoInfo;

    const closure = new Set<string>();
    for (const m of info.modules ?? []) {
        if (typeof m.local === "string" && m.local.length > 0) {
            closure.add(m.local);
        }
    }

    const packageRoots = new Set<string>();
    for (const [key, pkg] of Object.entries(info.npmPackages ?? {})) {
        if (typeof pkg?.localPath === "string" && pkg.localPath.length > 0) {
            packageRoots.add(pkg.localPath);
        } else {
            diagnostics.push(
                `unresolved npm package "${key}" — run \`deno cache\` first`,
            );
        }
    }

    return {
        closure: [...closure].sort(),
        packageRoots: [...packageRoots].sort(),
        visitedFiles: closure.size,
        visitedGenerators: entries.length,
        diagnostics,
    };
}
