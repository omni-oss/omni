/**
 * `omni remote-sources install` - prefetch and pin every remote source across
 * all subsystems into one shared, content-addressed store. Pinned to
 * `crates/omni_remote_source/*`, `crates/omni_remote_source_contributors/*`,
 * `crates/omni_api/src/operations/remote_source.rs`, and
 * `crates/omni_cli_core/src/commands/remote_source.rs`.
 */

import { readdirSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { describe, expect, it } from "vitest";
import {
    makeLocalGitRepoWithAnnotatedTag,
    makeWorkspace,
    runOmni,
    skipUnlessGitCliAvailable,
} from "@/harness";

const CLONE_TIMEOUT_MS = 60_000;

/** Count `store/git/<slug>/<commit>` directories across every slug. */
function countCommitDirs(storeGit: string): number {
    let slugs: ReturnType<typeof readdirSync>;
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

describe("+remote-sources @e2e (shared store)", {
    tags: ["remote-sources"],
}, () => {
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

            expect(ws.read(".omni/sources/lock.json")).toBe(lockAfterInstall);
            expect(countCommitDirs(storeGit)).toBe(1);
        },
        CLONE_TIMEOUT_MS,
    );
});
