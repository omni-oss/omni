/**
 * Data-source abstraction. A source enumerates available (version, target)
 * benchmark artifacts and hands back their raw, unparsed `data.json` blobs plus
 * provenance. It knows nothing about schemas, the normalized model, or charts.
 *
 * See DESIGN.md §4.
 */

/** Canonical OS/target identifier used throughout the pipeline. */
export type TargetId =
    | "x86_64-unknown-linux-gnu"
    | "x86_64-pc-windows-msvc"
    | "aarch64-apple-darwin"
    // Forward-compatible with new targets and the local-run sentinel.
    | (string & {});

/** One retrievable artifact: a single `data.json` for a (version, target). */
export interface RunRef {
    /** omni version, e.g. "0.4.1" (from the git tag) or a local sentinel. */
    version: string;
    target: TargetId;
    /** Opaque, source-specific handle used by `fetchRaw` (path, blob sha, url…). */
    locator: string;
    /** When the artifact was published, if the source knows it. */
    publishedAt?: string;
    /**
     * Provenance for a "view source at this build" link, when the source can
     * resolve it: the commit that produced this artifact and a browsable URL to
     * it. Optional — local runs and hosts that can't resolve it omit these.
     */
    commitSha?: string;
    sourceUrl?: string;
}

export interface DataSourceDescriptor {
    /** Stable id, e.g. "github", "local-fs". */
    id: string;
    displayName: string;
}

/** Optional filter applied when enumerating runs. */
export interface RunQuery {
    /** Restrict to these omni versions; omit ⇒ all discoverable versions. */
    versions?: string[];
    /** Restrict to these targets; omit ⇒ all targets. */
    targets?: TargetId[];
}

export interface DataSource {
    readonly descriptor: DataSourceDescriptor;

    /** List every (version, target) this source can serve, optionally filtered. */
    listRuns(query?: RunQuery): Promise<RunRef[]>;

    /** Fetch the raw `data.json` text for a ref. No parsing here. */
    fetchRaw(ref: RunRef): Promise<{ ref: RunRef; json: string }>;
}
