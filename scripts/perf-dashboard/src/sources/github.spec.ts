import { describe, expect, it } from "vitest";
import { normalize, parseSuite } from "../ingest";
import { GitHubDataSource } from "./github";

const API = "https://api.test/gh";
const RAW = "https://raw.test";

const TREE_TWO_TARGETS = {
    truncated: false,
    tree: [
        { path: "README.md", type: "blob" },
        { path: "x86_64-unknown-linux-gnu/data.json", type: "blob" },
        { path: "x86_64-unknown-linux-gnu/SUMMARY.md", type: "blob" },
        { path: "x86_64-pc-windows-msvc/data.json", type: "blob" },
        // Filtered out: unknown target and nested path.
        { path: "bogus-target/data.json", type: "blob" },
        { path: "x/y/data.json", type: "blob" },
    ],
};

const TREE_LINUX_ONLY = {
    truncated: false,
    tree: [{ path: "x86_64-unknown-linux-gnu/data.json", type: "blob" }],
};

const SUITE_JSON = JSON.stringify({
    name: "full",
    generatedAt: "2026-05-01T12:00:00.000Z",
    taskBenchVersion: "0.1.0",
    scenarios: [
        {
            name: "scale-300",
            result: {
                concurrency: 8,
                daemon: true,
                versions: { omni: "0.4.1" },
                platform: {
                    os: { platform: "linux", release: "6.8", arch: "x64" },
                },
                tools: [
                    {
                        tool: "omni",
                        task: "t2",
                        taskGraphSize: 900,
                        cold: {
                            runs: 1,
                            failures: 0,
                            executedMedian: 900,
                            stats: {
                                samples: [8000],
                                min: 8000,
                                max: 8000,
                                mean: 8000,
                                median: 8000,
                                stddev: 0,
                            },
                        },
                        warm: {
                            runs: 1,
                            failures: 0,
                            executedMedian: 0,
                            stats: {
                                samples: [700],
                                min: 700,
                                max: 700,
                                mean: 700,
                                median: 700,
                                stddev: 0,
                            },
                        },
                    },
                ],
            },
        },
    ],
});

type Route = [string | RegExp, unknown];

function mockFetch(routes: Route[]): typeof fetch {
    return (async (input: string | URL | Request) => {
        const url = input.toString();
        for (const [pattern, payload] of routes) {
            const match =
                typeof pattern === "string"
                    ? url.includes(pattern)
                    : pattern.test(url);
            if (match) {
                const body =
                    typeof payload === "string"
                        ? payload
                        : JSON.stringify(payload);
                return new Response(body, { status: 200 });
            }
        }
        return new Response("not found", { status: 404 });
    }) as unknown as typeof fetch;
}

const DEFAULT_ROUTES: Route[] = [
    [
        "/tags",
        [
            { name: "0.4.1", commit: { sha: "sha041" } },
            { name: "0.3.0", commit: { sha: "sha030" } },
        ],
    ],
    ["/commits/main", { sha: "shamain" }],
    ["/commits/0.4.1", { sha: "sha041" }],
    ["/git/trees/sha041", TREE_TWO_TARGETS],
    ["/git/trees/sha030", TREE_LINUX_ONLY],
    ["/git/trees/shamain", TREE_LINUX_ONLY],
    [/\/repos\/o\/r($|\?)/, { default_branch: "main" }],
    [RAW, SUITE_JSON],
];

function source(routes: Route[] = DEFAULT_ROUTES, extra = {}) {
    return new GitHubDataSource({
        owner: "o",
        repo: "r",
        apiBaseUrl: API,
        rawBaseUrl: RAW,
        fetchImpl: mockFetch(routes),
        ...extra,
    });
}

describe("GitHubDataSource", () => {
    it("discovers tags + default branch and probes targets per ref", async () => {
        const refs = await source().listRuns();
        const key = (r: { version: string; target: string }) =>
            `${r.version}:${r.target}`;
        const keys = refs.map(key).sort();

        expect(keys).toEqual([
            "0.3.0:x86_64-unknown-linux-gnu",
            "0.4.1:x86_64-pc-windows-msvc",
            "0.4.1:x86_64-unknown-linux-gnu",
            "main:x86_64-unknown-linux-gnu",
        ]);
    });

    it("filters unknown targets and nested paths from the tree", async () => {
        const refs = await source().listRuns();
        expect(refs.some((r) => r.target === "bogus-target")).toBe(false);
    });

    it("attaches commit SHA + browsable source URL, pinning raw to the SHA", async () => {
        const refs = await source().listRuns({ versions: ["0.4.1"] });
        const linux = refs.find((r) => r.target === "x86_64-unknown-linux-gnu");
        expect(linux?.commitSha).toBe("sha041");
        expect(linux?.sourceUrl).toBe("https://github.com/o/r/tree/sha041");
        expect(linux?.locator).toBe(
            `${RAW}/o/r/sha041/x86_64-unknown-linux-gnu/data.json`,
        );
    });

    it("filters by version and target via the query", async () => {
        const byVersion = await source().listRuns({ versions: ["0.3.0"] });
        expect(byVersion).toHaveLength(1);
        expect(byVersion[0]?.version).toBe("0.3.0");

        const byTarget = await source().listRuns({
            targets: ["x86_64-pc-windows-msvc"],
        });
        expect(byTarget).toEqual([
            expect.objectContaining({
                version: "0.4.1",
                target: "x86_64-pc-windows-msvc",
            }),
        ]);
    });

    it("scans a single ref when configured, resolving its SHA", async () => {
        const refs = await source(DEFAULT_ROUTES, { ref: "0.4.1" }).listRuns();
        expect(refs.every((r) => r.version === "0.4.1")).toBe(true);
        expect(refs).toHaveLength(2); // linux + windows
    });

    it("fetchRaw returns the payload text, and it round-trips through normalize", async () => {
        const src = source();
        const [ref] = await src.listRuns({ versions: ["0.4.1"] });
        if (!ref) throw new Error("expected a ref");
        const { json } = await src.fetchRaw(ref);

        const [run] = normalize(ref, parseSuite(json), src.descriptor.id);
        expect(run?.source).toBe("github");
        expect(run?.commitSha).toBe("sha041");
        expect(run?.points.length).toBeGreaterThan(0);
    });

    it("throws a descriptive error on a failed API call", async () => {
        // Empty routes ⇒ every request 404s; the tags call fails first.
        const src = new GitHubDataSource({
            owner: "o",
            repo: "r",
            apiBaseUrl: API,
            rawBaseUrl: RAW,
            includeDefaultBranch: false,
            fetchImpl: mockFetch([]),
        });
        await expect(src.listRuns()).rejects.toThrow(/GitHub API 404/);
    });
});
