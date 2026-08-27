/**
 * `omni projection` - workspace projections end to end: a local source whose
 * files are materialized into a workspace destination via links, tracked in the
 * `.omni/sources/projection/links.json` ledger for idempotent sync and safe
 * teardown. Pinned to `crates/omni_projections/*`,
 * `crates/omni_api/src/operations/projection.rs`, and
 * `crates/omni_cli_core/src/commands/projection.rs`.
 */

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
});
