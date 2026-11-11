/**
 * `omni remote-sources install` - prefetch and pin every remote source across
 * all subsystems into one shared, content-addressed store. Pinned to
 * `crates/omni_remote_source/*`, `crates/omni_remote_source_contributors/*`,
 * `crates/omni_api/src/operations/remote_source.rs`, and
 * `crates/omni_cli_core/src/commands/remote_source.rs`.
 */

import {
    existsSync,
    mkdtempSync,
    readdirSync,
    rmSync,
    type Dirent,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { describe, expect, it, onTestFinished } from "vitest";

import {
    makeLocalGitRepoWithAnnotatedTag,
    makeWorkspace,
    runOmni,
    skipUnlessGitCliAvailable,
} from "@/harness";

const CLONE_TIMEOUT_MS = 60_000;

/** Count `store/git/<slug>/<commit>` directories across every slug. */
function countCommitDirs(storeGit: string): number {
    let slugs: Dirent<string>[];
    try {
        slugs = readdirSync(storeGit, { withFileTypes: true });
    } catch {
        return 0;
    }

    let count = 0;
    for (const slug of slugs) {
        if (!slug.isDirectory()) continue;
        try {
            count += readdirSync(join(storeGit, slug.name), {
                withFileTypes: true,
            }).filter((d) => d.isDirectory()).length;
        } catch {
            // A non-directory entry under the slug is ignored.
        }
    }
    return count;
}

describe(
    "+remote-sources @e2e (shared store)",
    {
        tags: ["remote-sources"],
    },
    () => {
        it(
            "install fetches a shared source once and a later sync reuses it",
            async (ctx) => {
                await skipUnlessGitCliAvailable(ctx);

                const repo = await makeLocalGitRepoWithAnnotatedTag({
                    files: {
                        "generator.omni.yaml": "name: shared\n",
                        "content.md": "# shared\n",
                    },
                });
                const uri = pathToFileURL(repo.url).href;

                // The same repo is referenced by both a generator source and a
                // projection source, so the second subsystem must dedupe.
                const ws = makeWorkspace({
                    workspace: {
                        projects: ["**"],
                        generators: [{ source: "git", uri, rev: "main" }],
                        projections: [
                            {
                                source: "git",
                                uri,
                                rev: "main",
                                id: "shared",
                                routes: [
                                    {
                                        strategy: "mirror",
                                        allow_git: true,
                                        allow_omni_config: true,
                                        target: "@workspace/.agents/shared",
                                    },
                                ],
                            },
                        ],
                    },
                });

                const install = await runOmni(["remote-sources", "install"], {
                    cwd: ws.cwd,
                    timeout: CLONE_TIMEOUT_MS,
                });
                expect(install).toHaveSucceeded();
                expect(install).toOutputContaining("materialized");
                expect(install).toOutputContaining("deduplicated");

                // The single committed lockfile lives at .omni/sources/lock.json,
                // and the shared store holds exactly one commit-keyed checkout.
                expect(ws.exists(".omni/sources/lock.json")).toBe(true);
                const storeGit = ws.path(".omni/sources/store/git");
                expect(countCommitDirs(storeGit)).toBe(1);

                const lockAfterInstall = ws.read(".omni/sources/lock.json");

                // A following projection sync is a cache hit: it applies the mirror
                // but re-clones nothing and leaves the lockfile byte-identical.
                const sync = await runOmni(["projection", "sync"], {
                    cwd: ws.cwd,
                    timeout: CLONE_TIMEOUT_MS,
                });
                expect(sync).toHaveSucceeded();
                expect(ws.exists(".agents/shared/content.md")).toBe(true);

                expect(ws.read(".omni/sources/lock.json")).toBe(
                    lockAfterInstall,
                );
                expect(countCommitDirs(storeGit)).toBe(1);
            },
            CLONE_TIMEOUT_MS,
        );

        it(
            "finds a git source's own projection.omni.yaml when ignore sync hides the store",
            async (ctx) => {
                await skipUnlessGitCliAvailable(ctx);

                const repo = await makeLocalGitRepoWithAnnotatedTag({
                    files: {
                        "projection.omni.yaml": [
                            'name: "@vendor/skills"',
                            "routes:",
                            "  - strategy: mirror",
                            '    scope: "*.md"',
                            '    target: "@workspace/.agents/skills"',
                            "",
                        ].join("\n"),
                        "rust.md": "# rust\n",
                    },
                });
                const uri = pathToFileURL(repo.url).href;

                // No workspace routes: they must come from the source's manifest.
                const ws = makeWorkspace({
                    workspace: {
                        projects: ["**"],
                        projections: [
                            {
                                source: "git",
                                uri,
                                rev: "main",
                                id: "vendor-skills",
                            },
                        ],
                    },
                });

                // `ignore sync` writes `/.omni/sources/**`, which hides the
                // source store from walks that honor the workspace's ignores.
                expect(
                    await runOmni(["ignore", "sync"], { cwd: ws.cwd }),
                ).toHaveSucceeded();
                expect(ws.read(".omniignore")).toContain("/.omni/sources/**");

                const sync = await runOmni(["projection", "sync", "--update"], {
                    cwd: ws.cwd,
                    timeout: CLONE_TIMEOUT_MS,
                });
                expect(sync).toHaveSucceeded();
                // A git checkout may convert line endings (Windows autocrlf).
                expect(ws.read(".agents/skills/rust.md").trim()).toBe("# rust");
            },
            CLONE_TIMEOUT_MS,
        );

        it(
            "finds a git source's generators and tools when ignore sync hides the store",
            async (ctx) => {
                await skipUnlessGitCliAvailable(ctx);

                const repo = await makeLocalGitRepoWithAnnotatedTag({
                    files: {
                        "generator.omni.yaml":
                            "name: remote-gen\ndescription: from a git source\n",
                        "tool.omni.yaml": [
                            "type: js",
                            "name: remote-tool",
                            "entrypoint: ./index.mjs",
                            "",
                        ].join("\n"),
                        "index.mjs": "export default () => {};\n",
                    },
                });
                const uri = pathToFileURL(repo.url).href;

                const ws = makeWorkspace({
                    workspace: {
                        projects: ["**"],
                        generators: [{ source: "git", uri, rev: "main" }],
                        tools: [{ source: "git", uri, rev: "main" }],
                    },
                });

                // `ignore sync` writes `/.omni/sources/**`, which hides the
                // source store from walks that honor the workspace's ignores.
                expect(
                    await runOmni(["ignore", "sync"], { cwd: ws.cwd }),
                ).toHaveSucceeded();
                expect(ws.read(".omniignore")).toContain("/.omni/sources/**");

                const generators = await runOmni(["generator", "list"], {
                    cwd: ws.cwd,
                    timeout: CLONE_TIMEOUT_MS,
                });
                expect(generators).toHaveSucceeded();
                expect(generators).toOutputContaining("remote-gen");

                const tools = await runOmni(["tool", "list"], {
                    cwd: ws.cwd,
                    timeout: CLONE_TIMEOUT_MS,
                });
                expect(tools).toHaveSucceeded();
                expect(tools).toOutputContaining("remote-tool");
            },
            CLONE_TIMEOUT_MS,
        );

        it(
            "workspaces sharing one store never delete each other's checkouts, and a deleted store is refetched",
            async (ctx) => {
                await skipUnlessGitCliAvailable(ctx);

                const repo = await makeLocalGitRepoWithAnnotatedTag({
                    files: { "generator.omni.yaml": "name: shared\n" },
                });
                const uri = pathToFileURL(repo.url).href;
                const store = join(
                    mkdtempSync(join(tmpdir(), "omni-e2e-store-")),
                    "sources",
                );
                onTestFinished(() =>
                    rmSync(join(store, ".."), { recursive: true, force: true }),
                );
                const env = { OMNI_SOURCES_DIR: store };

                const withSource = makeWorkspace({
                    workspace: {
                        projects: ["**"],
                        generators: [{ source: "git", uri, rev: "main" }],
                    },
                });
                const withoutSource = makeWorkspace({
                    workspace: { projects: ["**"] },
                });

                expect(
                    await runOmni(["remote-sources", "install"], {
                        cwd: withSource.cwd,
                        env,
                        timeout: CLONE_TIMEOUT_MS,
                    }),
                ).toHaveSucceeded();
                expect(countCommitDirs(join(store, "git"))).toBe(1);
                // Nothing lands in the workspace's own store.
                expect(withSource.exists(".omni/sources/store")).toBe(false);

                // An install in a workspace that references nothing used to
                // garbage-collect every checkout it did not know about.
                expect(
                    await runOmni(["remote-sources", "install"], {
                        cwd: withoutSource.cwd,
                        env,
                    }),
                ).toHaveSucceeded();
                expect(countCommitDirs(join(store, "git"))).toBe(1);

                // The store is a disposable cache: delete it and the next use
                // refetches the pinned commit.
                // Workspace ignore rules must not hide a store outside the
                // workspace either.
                expect(
                    await runOmni(["ignore", "sync"], { cwd: withSource.cwd }),
                ).toHaveSucceeded();
                rmSync(store, { recursive: true, force: true });
                const listed = await runOmni(["generator", "list"], {
                    cwd: withSource.cwd,
                    env,
                    timeout: CLONE_TIMEOUT_MS,
                });
                expect(listed).toHaveSucceeded();
                expect(listed).toOutputContaining("shared");
                expect(countCommitDirs(join(store, "git"))).toBe(1);
            },
            CLONE_TIMEOUT_MS,
        );

        it(
            "sources.dir in worktree.omni.yaml selects the store, and --sources-dir overrides it",
            async (ctx) => {
                await skipUnlessGitCliAvailable(ctx);

                const repo = await makeLocalGitRepoWithAnnotatedTag({
                    files: { "generator.omni.yaml": "name: shared\n" },
                });
                const uri = pathToFileURL(repo.url).href;
                const ws = makeWorkspace({
                    workspace: {
                        projects: ["**"],
                        generators: [{ source: "git", uri, rev: "main" }],
                    },
                });
                ws.write("worktree.omni.yaml", "sources:\n  dir: .store\n");
                // `undefined` lets the worktree file decide instead of the
                // harness's per-workspace isolation.
                const env = { OMNI_SOURCES_DIR: undefined };

                expect(
                    await runOmni(["remote-sources", "install"], {
                        cwd: ws.cwd,
                        env,
                        timeout: CLONE_TIMEOUT_MS,
                    }),
                ).toHaveSucceeded();
                expect(countCommitDirs(ws.path(".store/git"))).toBe(1);

                const flagDir = join(
                    mkdtempSync(join(tmpdir(), "omni-e2e-flag-")),
                    "s",
                );
                onTestFinished(() =>
                    rmSync(join(flagDir, ".."), {
                        recursive: true,
                        force: true,
                    }),
                );
                expect(
                    await runOmni(
                        ["--sources-dir", flagDir, "remote-sources", "install"],
                        { cwd: ws.cwd, env, timeout: CLONE_TIMEOUT_MS },
                    ),
                ).toHaveSucceeded();
                expect(countCommitDirs(join(flagDir, "git"))).toBe(1);
                expect(existsSync(ws.path(".omni/sources/store"))).toBe(false);
            },
            CLONE_TIMEOUT_MS,
        );

        it(
            "a projection from a deleted store is repaired by the next sync, whichever link kind auto chose",
            async (ctx) => {
                await skipUnlessGitCliAvailable(ctx);

                const repo = await makeLocalGitRepoWithAnnotatedTag({
                    files: { "content.md": "# shared\n" },
                });
                const uri = pathToFileURL(repo.url).href;
                const ws = makeWorkspace({
                    workspace: {
                        projects: ["**"],
                        projections: [
                            {
                                source: "git",
                                uri,
                                rev: "main",
                                id: "shared",
                                routes: [
                                    {
                                        strategy: "mirror",
                                        allow_git: true,
                                        allow_omni_config: true,
                                        target: "@workspace/.agents/shared",
                                    },
                                ],
                            },
                        ],
                    },
                });
                const sync = () =>
                    runOmni(["projection", "sync"], {
                        cwd: ws.cwd,
                        timeout: CLONE_TIMEOUT_MS,
                    });

                expect(await sync()).toHaveSucceeded();
                expect(ws.read(".agents/shared/content.md").trim()).toBe(
                    "# shared",
                );

                // `auto` links to the store (symlink) or owns independent files
                // (reflink) depending on the filesystem; either way deleting the
                // store must not leave the workspace broken after a sync.
                rmSync(ws.path(".omni/sources/store"), {
                    recursive: true,
                    force: true,
                });
                expect(await sync()).toHaveSucceeded();
                expect(ws.read(".agents/shared/content.md").trim()).toBe(
                    "# shared",
                );
                expect(
                    await runOmni(["projection", "status"], { cwd: ws.cwd }),
                ).toHaveSucceeded();
            },
            CLONE_TIMEOUT_MS,
        );

        it(
            "prune is the only command that deletes from the store, and what it deletes is refetched",
            async (ctx) => {
                await skipUnlessGitCliAvailable(ctx);

                const repo = await makeLocalGitRepoWithAnnotatedTag({
                    files: { "generator.omni.yaml": "name: shared\n" },
                });
                const uri = pathToFileURL(repo.url).href;
                const ws = makeWorkspace({
                    workspace: {
                        projects: ["**"],
                        generators: [{ source: "git", uri, rev: "main" }],
                    },
                });
                const storeGit = ws.path(".omni/sources/store/git");
                const run = (args: string[]) =>
                    runOmni(["remote-sources", ...args], {
                        cwd: ws.cwd,
                        timeout: CLONE_TIMEOUT_MS,
                    });

                expect(await run(["install"])).toHaveSucceeded();
                expect(countCommitDirs(storeGit)).toBe(1);

                // A prune with no filter is refused rather than deleting all.
                const refused = await run(["prune", "--yes"]);
                expect(refused.exitCode).not.toBe(0);
                expect(countCommitDirs(storeGit)).toBe(1);

                // Dry run lists and deletes nothing.
                const dry = await run(["prune", "--all", "--dry-run"]);
                expect(dry).toHaveSucceeded();
                expect(dry).toOutputContaining("would prune 1 checkouts");
                expect(countCommitDirs(storeGit)).toBe(1);

                // A recent checkout is not older than a day.
                const recent = await run([
                    "prune",
                    "--older-than",
                    "1d",
                    "--yes",
                ]);
                expect(recent).toHaveSucceeded();
                expect(countCommitDirs(storeGit)).toBe(1);

                const pruned = await run(["prune", "--all", "--yes"]);
                expect(pruned).toHaveSucceeded();
                expect(pruned).toOutputContaining("Pruned 1 checkouts");
                expect(countCommitDirs(storeGit)).toBe(0);

                // The pin stays in the lockfile, so the next use refetches.
                const listed = await runOmni(["generator", "list"], {
                    cwd: ws.cwd,
                    timeout: CLONE_TIMEOUT_MS,
                });
                expect(listed).toHaveSucceeded();
                expect(listed).toOutputContaining("shared");
                expect(countCommitDirs(storeGit)).toBe(1);
            },
            CLONE_TIMEOUT_MS,
        );
    },
);
