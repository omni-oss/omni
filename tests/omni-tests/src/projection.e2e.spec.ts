/**
 * `omni projection` - workspace projections end to end: a local source whose
 * files are materialized into a workspace destination via links, tracked in the
 * `.omni/sources/projection/links.json` ledger for idempotent sync and safe
 * teardown. Pinned to `crates/omni_projections/*`,
 * `crates/omni_api/src/operations/projection.rs`, and
 * `crates/omni_cli_core/src/commands/projection.rs`.
 */

import { rmSync, writeFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

function projectionWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            projections: [
                {
                    source: "local",
                    path: "./vendor/skills",
                    id: "local-skills",
                    projections: [
                        {
                            strategy: "mirror",
                            target: "@workspace/.agents/skills",
                        },
                    ],
                },
            ],
        },
        files: {
            "vendor/skills/rust.md": "# rust\n",
            "vendor/skills/sub/python.md": "# python\n",
        },
    };
}

// A `namespaced` projection links the whole source tree under `target/<id>`,
// e.g. installing a package into `node_modules/<id>`.
function namespacedWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            projections: [
                {
                    source: "local",
                    path: "./vendor/pkg",
                    id: "my-pkg",
                    projections: [
                        {
                            strategy: "namespaced",
                            target: "@workspace/node_modules",
                        },
                    ],
                },
            ],
        },
        files: {
            "vendor/pkg/index.js": "module.exports = 1;\n",
            "vendor/pkg/lib/util.js": "module.exports = 2;\n",
        },
    };
}

function ledgerLinkCount(ws: ReturnType<typeof makeWorkspace>): number {
    const ledger = JSON.parse(
        ws.read(".omni/sources/projection/links.json"),
    ) as { links: unknown[] };
    return ledger.links.length;
}

describe("+projection @e2e", { tags: ["projection"] }, () => {
    it("materializes a local source into the target on sync", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();

        // The mirrored files resolve to the source content through the links.
        expect(ws.read(".agents/skills/rust.md")).toBe("# rust\n");
        expect(ws.read(".agents/skills/sub/python.md")).toBe("# python\n");
        // The ledger is recorded for later idempotent syncs / teardown.
        expect(ws.exists(".omni/sources/projection/links.json")).toBe(true);
    });

    it("plans without writing on --dry-run", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        const result = await runOmni(["projection", "sync", "--dry-run"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain(".agents/skills/rust.md");
        expect(ws.exists(".agents/skills/rust.md")).toBe(false);
    });

    it("is idempotent: a second sync leaves links up-to-date", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        const first = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(first).toHaveSucceeded();

        const second = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second.stdout).toContain("up-to-date");
        expect(ws.read(".agents/skills/rust.md")).toBe("# rust\n");
    });

    it("reports link health with status", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        await runOmni(["projection", "sync"], { cwd: ws.cwd });
        const status = await runOmni(["projection", "status", "--verbose"], {
            cwd: ws.cwd,
        });
        expect(status).toHaveSucceeded();
        // Two files mirrored, all present.
        expect(status.stdout).toContain("2 ok");
    });

    it("tears down a source's links with unlink", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(ws.exists(".agents/skills/rust.md")).toBe(true);

        const result = await runOmni(["projection", "unlink", "local-skills"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveSucceeded();
        expect(ws.exists(".agents/skills/rust.md")).toBe(false);
        expect(ws.exists(".agents/skills/sub/python.md")).toBe(false);
    });

    it("prunes only the ledger links that have gone dangling", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(ledgerLinkCount(ws)).toBe(2);

        // Deleting the source turns its mirrored symlink into a dangling link,
        // while the other file's link stays healthy.
        rmSync(ws.path("vendor/skills/rust.md"));

        const result = await runOmni(["projection", "prune"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain("1 dangling link(s) removed");

        // Prune dropped exactly the dangling entry and left the healthy one.
        expect(ledgerLinkCount(ws)).toBe(1);
        expect(ws.read(".agents/skills/sub/python.md")).toBe("# python\n");
    });

    it("installs a whole source under target/<id> with the namespaced strategy", async () => {
        const ws = makeWorkspace(namespacedWorkspace());

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();

        // A single directory link exposes the entire source tree under the id.
        expect(ws.read("node_modules/my-pkg/index.js")).toBe(
            "module.exports = 1;\n",
        );
        expect(ws.read("node_modules/my-pkg/lib/util.js")).toBe(
            "module.exports = 2;\n",
        );

        const status = await runOmni(["projection", "status"], { cwd: ws.cwd });
        expect(status).toHaveSucceeded();
        // Namespaced emits one directory link, so status reports a single entry.
        expect(status.stdout).toContain("1 ok");
    });

    it("repairs a drifted link only when sync is forced", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        await runOmni(["projection", "sync"], { cwd: ws.cwd });

        // Replace the managed symlink with a real file: the link has drifted
        // away from what the ledger recorded.
        rmSync(ws.path(".agents/skills/rust.md"));
        writeFileSync(ws.path(".agents/skills/rust.md"), "local edit\n");

        const drifted = await runOmni(["projection", "status", "--verbose"], {
            cwd: ws.cwd,
        });
        expect(drifted).toHaveSucceeded();
        expect(drifted.stdout).toContain("1 drifted");

        // A plain sync leaves the drifted destination untouched (pin unchanged).
        await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(ws.read(".agents/skills/rust.md")).toBe("local edit\n");

        // --force re-applies every link, restoring the projection.
        const forced = await runOmni(["projection", "sync", "--force"], {
            cwd: ws.cwd,
        });
        expect(forced).toHaveSucceeded();
        expect(ws.read(".agents/skills/rust.md")).toBe("# rust\n");

        const healthy = await runOmni(["projection", "status"], {
            cwd: ws.cwd,
        });
        expect(healthy.stdout).toContain("2 ok");
    });
});
