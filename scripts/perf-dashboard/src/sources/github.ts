import { fetchWithRetry } from "../http";
import { isKnownTarget } from "./target";
import type {
    DataSource,
    DataSourceDescriptor,
    RunQuery,
    RunRef,
    TargetId,
} from "./types";

/**
 * Reads the published performance-comparison repo (a `gh-pages`-style repo
 * where each omni version is a git tag). Discovery is tag-driven, so the set of
 * tags *is* the version axis; the default branch is included as a floating
 * pseudo-version. Resolves each ref's commit SHA + a browsable source URL for
 * the "view source at this build" links. See DESIGN.md §4 & §7.2.
 *
 * Layout probed per ref:
 *   <ref>/<target>/data.json   (+ SUMMARY.md, README.md — ignored here)
 */
export interface GitHubConfig {
    owner: string;
    repo: string;
    /** Restrict to a single ref (tag/branch/sha); omit ⇒ all tags (+ default branch). */
    ref?: string;
    /** Personal/installation token; optional for public repos (lifts rate limits). */
    token?: string;
    /** Include the default branch as a floating pseudo-version. Default true. */
    includeDefaultBranch?: boolean;
    /** Label for the default-branch pseudo-version. Default "main". */
    defaultBranchLabel?: string;
    /** API base. Default "https://api.github.com". */
    apiBaseUrl?: string;
    /** Raw content base. Default "https://raw.githubusercontent.com". */
    rawBaseUrl?: string;
    /** Injectable fetch (for tests). Defaults to the global `fetch`. */
    fetchImpl?: typeof fetch;
}

interface ResolvedRef {
    version: string;
    /** ref name (tag/branch) as published. */
    ref: string;
    sha: string;
}

interface GitHubTag {
    name: string;
    commit: { sha: string };
}

interface GitTree {
    tree: Array<{ path: string; type: string }>;
    truncated: boolean;
}

const DATA_FILE_SUFFIX = "/data.json";

export class GitHubDataSource implements DataSource {
    readonly descriptor: DataSourceDescriptor = {
        id: "github",
        displayName: "GitHub",
    };

    private readonly apiBase: string;
    private readonly rawBase: string;
    private readonly fetchImpl: typeof fetch;

    constructor(private readonly config: GitHubConfig) {
        this.apiBase = (config.apiBaseUrl ?? "https://api.github.com").replace(
            /\/+$/,
            "",
        );
        this.rawBase = (
            config.rawBaseUrl ?? "https://raw.githubusercontent.com"
        ).replace(/\/+$/, "");
        this.fetchImpl = config.fetchImpl ?? globalThis.fetch;
    }

    async listRuns(query?: RunQuery): Promise<RunRef[]> {
        const refs = await this.discoverRefs();
        const refined = query?.versions
            ? refs.filter((r) => query.versions?.includes(r.version))
            : refs;

        const out: RunRef[] = [];
        for (const resolved of refined) {
            const targets = await this.probeTargets(resolved.sha);
            for (const target of targets) {
                if (query?.targets && !query.targets.includes(target)) continue;
                out.push(this.toRunRef(resolved, target));
            }
        }
        return out;
    }

    async fetchRaw(ref: RunRef): Promise<{ ref: RunRef; json: string }> {
        const json = await this.httpText(ref.locator);
        return { ref, json };
    }

    // --- discovery -------------------------------------------------------

    private async discoverRefs(): Promise<ResolvedRef[]> {
        if (this.config.ref) {
            const sha = await this.resolveCommitSha(this.config.ref);
            return [{ version: this.config.ref, ref: this.config.ref, sha }];
        }

        const refs: ResolvedRef[] = [];
        const seen = new Set<string>();
        for (const tag of await this.listTags()) {
            refs.push({
                version: tag.name,
                ref: tag.name,
                sha: tag.commit.sha,
            });
            seen.add(tag.name);
        }

        if (this.config.includeDefaultBranch !== false) {
            const label = this.config.defaultBranchLabel ?? "main";
            if (!seen.has(label)) {
                const branch = await this.defaultBranch();
                const sha = await this.resolveCommitSha(branch);
                refs.push({ version: label, ref: branch, sha });
            }
        }
        return refs;
    }

    private async listTags(): Promise<GitHubTag[]> {
        const perPage = 100;
        const all: GitHubTag[] = [];
        for (let page = 1; page <= 20; page++) {
            const batch = await this.httpJson<GitHubTag[]>(
                `${this.repoApi()}/tags?per_page=${perPage}&page=${page}`,
            );
            all.push(...batch);
            if (batch.length < perPage) break;
        }
        return all;
    }

    private async defaultBranch(): Promise<string> {
        const repo = await this.httpJson<{ default_branch: string }>(
            this.repoApi(),
        );
        return repo.default_branch;
    }

    private async resolveCommitSha(ref: string): Promise<string> {
        const commit = await this.httpJson<{ sha: string }>(
            `${this.repoApi()}/commits/${encodeURIComponent(ref)}`,
        );
        return commit.sha;
    }

    /** Which known targets have a `<target>/data.json` at this commit. */
    private async probeTargets(sha: string): Promise<TargetId[]> {
        const tree = await this.httpJson<GitTree>(
            `${this.repoApi()}/git/trees/${sha}?recursive=1`,
        );
        const targets = new Set<TargetId>();
        for (const entry of tree.tree) {
            if (entry.type !== "blob") continue;
            if (!entry.path.endsWith(DATA_FILE_SUFFIX)) continue;
            const dir = entry.path.slice(0, -DATA_FILE_SUFFIX.length);
            // Only top-level `<target>/data.json`, and only known targets.
            if (dir.includes("/")) continue;
            if (isKnownTarget(dir)) targets.add(dir);
        }
        return [...targets].sort((a, b) => a.localeCompare(b));
    }

    private toRunRef(resolved: ResolvedRef, target: TargetId): RunRef {
        const { owner, repo } = this.config;
        return {
            version: resolved.version,
            target,
            // Pin the raw URL to the immutable commit SHA.
            locator: `${this.rawBase}/${owner}/${repo}/${resolved.sha}/${target}/data.json`,
            commitSha: resolved.sha,
            sourceUrl: `https://github.com/${owner}/${repo}/tree/${resolved.sha}`,
        };
    }

    // --- http ------------------------------------------------------------

    private repoApi(): string {
        return `${this.apiBase}/repos/${this.config.owner}/${this.config.repo}`;
    }

    private headers(accept: string): Record<string, string> {
        const h: Record<string, string> = { Accept: accept };
        if (this.config.token) {
            h.Authorization = `Bearer ${this.config.token}`;
            h["X-GitHub-Api-Version"] = "2022-11-28";
        }
        return h;
    }

    private async httpJson<T>(url: string): Promise<T> {
        const res = await fetchWithRetry(
            url,
            { headers: this.headers("application/vnd.github+json") },
            { fetchImpl: this.fetchImpl },
        );
        if (!res.ok) {
            throw new Error(
                `GitHub API ${res.status} for ${url}: ${await safeBody(res)}`,
            );
        }
        return (await res.json()) as T;
    }

    private async httpText(url: string): Promise<string> {
        const res = await fetchWithRetry(
            url,
            { headers: this.headers("application/vnd.github.raw+json") },
            { fetchImpl: this.fetchImpl },
        );
        if (!res.ok) {
            throw new Error(
                `GitHub raw ${res.status} for ${url}: ${await safeBody(res)}`,
            );
        }
        return await res.text();
    }
}

async function safeBody(res: Response): Promise<string> {
    try {
        return (await res.text()).slice(0, 200);
    } catch {
        return "<no body>";
    }
}
