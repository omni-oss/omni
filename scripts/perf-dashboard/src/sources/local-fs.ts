import { readdir, readFile, stat } from "node:fs/promises";
import { join, resolve, sep } from "node:path";
import { isKnownTarget, osArchToTarget } from "./target";
import type {
    DataSource,
    DataSourceDescriptor,
    RunQuery,
    RunRef,
    TargetId,
} from "./types";

/**
 * Feeds the dashboard directly from local benchmark output — e.g.
 * `task-bench suite --json data.json` — without needing the published
 * `<version>/<target>/` repo layout. See DESIGN.md §4 ("Local disk backend").
 */
export interface LocalFsConfig {
    /**
     * Files and/or directories to read. Each entry may be a single `data.json`,
     * a directory of `*.json` runs, or a published-style `<version>/<target>/`
     * tree. Directories are searched recursively for `*.json`.
     */
    paths: string[];
    /** Explicit version override (wins over path/payload inference). */
    version?: string;
    /** Explicit target override (wins over path/payload inference). */
    target?: TargetId;
}

/** Minimal shape peeked from a payload for coordinate inference. */
interface PayloadPeek {
    versions?: { omni?: string | null } | undefined;
    platform?: { os?: { platform?: string; arch?: string } } | undefined;
}

export class LocalFsDataSource implements DataSource {
    readonly descriptor: DataSourceDescriptor = {
        id: "local-fs",
        displayName: "Local disk",
    };

    constructor(private readonly config: LocalFsConfig) {}

    async listRuns(query?: RunQuery): Promise<RunRef[]> {
        const files = await collectJsonFiles(this.config.paths);
        const refs: RunRef[] = [];
        for (const file of files) {
            const ref = await this.deriveRef(file);
            if (query?.versions && !query.versions.includes(ref.version)) {
                continue;
            }
            if (query?.targets && !query.targets.includes(ref.target)) {
                continue;
            }
            refs.push(ref);
        }
        return refs;
    }

    async fetchRaw(ref: RunRef): Promise<{ ref: RunRef; json: string }> {
        return { ref, json: await readFile(ref.locator, "utf8") };
    }

    /** Resolve a (version, target) coordinate for a local file. */
    private async deriveRef(file: string): Promise<RunRef> {
        const abs = resolve(file);
        let version = this.config.version;
        let target = this.config.target;

        // (2) Path inference: .../<version>/<target>/<file>.json
        if (!version || !target) {
            const parts = abs.split(sep);
            const maybeTarget = parts[parts.length - 2];
            const maybeVersion = parts[parts.length - 3];
            if (maybeTarget && isKnownTarget(maybeTarget)) {
                target ??= maybeTarget;
                if (maybeVersion) version ??= maybeVersion;
            }
        }

        // (3) Payload inference: omni version + os/arch → target.
        if (!version || !target) {
            const peek = await this.peekPayload(abs);
            if (peek) {
                version ??= peek.versions?.omni ?? undefined;
                const os = peek.platform?.os;
                if (!target && os?.platform && os.arch) {
                    target = osArchToTarget(os.platform, os.arch) ?? undefined;
                }
            }
        }

        // (4) Last resort: a stable local sentinel, tagged by mtime.
        if (!version) {
            const st = await stat(abs);
            version = `local@${Math.round(st.mtimeMs)}`;
        }

        return { version, target: target ?? "local", locator: abs };
    }

    private async peekPayload(abs: string): Promise<PayloadPeek | null> {
        try {
            const raw = JSON.parse(await readFile(abs, "utf8"));
            const result = raw?.scenarios?.[0]?.result;
            return result ? (result as PayloadPeek) : null;
        } catch {
            return null;
        }
    }
}

/** Recursively collect `*.json` files from the given files/directories. */
async function collectJsonFiles(paths: string[]): Promise<string[]> {
    const out: string[] = [];
    for (const p of paths) {
        const st = await stat(p).catch(() => null);
        if (!st) continue;
        if (st.isDirectory()) {
            await walkDir(p, out);
        } else if (p.endsWith(".json")) {
            out.push(p);
        }
    }
    // Deterministic order so listRuns is stable across runs.
    return [...new Set(out.map((f) => resolve(f)))].sort((a, b) =>
        a.localeCompare(b),
    );
}

async function walkDir(dir: string, out: string[]): Promise<void> {
    const entries = await readdir(dir, { withFileTypes: true });
    for (const entry of entries) {
        const full = join(dir, entry.name);
        if (entry.isDirectory()) {
            await walkDir(full, out);
        } else if (entry.isFile() && entry.name.endsWith(".json")) {
            out.push(full);
        }
    }
}
