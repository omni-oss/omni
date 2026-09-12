/**
 * `omni ignore sync` / `omni ignore clean` - maintaining omni's single fenced,
 * managed block in the configured ignore files (`.gitignore`, `.ignore`,
 * `.omniignore` by default). The block lists omni's own `.omni/` state plus one
 * anchored pattern per projection link read from the ledger. Pinned to
 * `crates/omni_ignore_core/*`, `crates/omni_ignore_contributors/*`,
 * `crates/omni_api/src/operations/ignore.rs`, and
 * `crates/omni_cli_core/src/commands/ignore.rs`.
 */

import { describe, expect, it } from "vitest";
import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

const BEGIN = "# @@omni-managed:begin";
const END = "# @@omni-managed:end";

function projectionWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            projections: [
                {
                    source: "local",
                    path: "./vendor/skills",
                    id: "local-skills",
                    routes: [
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
        },
    };
}

describe("+ignore @e2e", { tags: ["ignore"] }, () => {
    it("writes the managed block into every default file after a projection sync", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        expect(await runOmni(["projection", "sync"], { cwd: ws.cwd })).toHaveSucceeded();
        expect(await runOmni(["ignore", "sync"], { cwd: ws.cwd })).toHaveSucceeded();

        for (const file of [".gitignore", ".ignore", ".omniignore"]) {
            const content = ws.read(file);
            expect(content).toContain(BEGIN);
            expect(content).toContain(END);
            // Internal `.omni/` state, ordered with the keep-rule after its base.
            expect(content).toContain("/.omni/cache/**");
            expect(content).toContain("/.omni/sources/*/**");
            expect(content).toContain("!/.omni/sources/*/lock.json");
            // One anchored pattern per projection link from the ledger.
            expect(content).toContain("/.agents/skills/rust.md");
        }
    });

    it("prints the block without writing on --dry-run", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        const result = await runOmni(["ignore", "sync", "--dry-run"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain(BEGIN);
        expect(result.stdout).toContain("/.omni/cache/**");
        expect(ws.exists(".gitignore")).toBe(false);
    });

    it("is idempotent: a second sync reports every file unchanged", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        expect(await runOmni(["ignore", "sync"], { cwd: ws.cwd })).toHaveSucceeded();
        const before = ws.read(".gitignore");

        const second = await runOmni(["ignore", "sync"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second.stdout).toContain("unchanged");
        expect(ws.read(".gitignore")).toBe(before);
    });

    it("--check passes when fresh and fails after a projection is added", async () => {
        const ws = makeWorkspace(projectionWorkspace());

        expect(await runOmni(["projection", "sync"], { cwd: ws.cwd })).toHaveSucceeded();
        expect(await runOmni(["ignore", "sync"], { cwd: ws.cwd })).toHaveSucceeded();

        const fresh = await runOmni(["ignore", "sync", "--check"], {
            cwd: ws.cwd,
        });
        expect(fresh).toHaveSucceeded();

        // A new source file becomes a new ledger link, so the rendered block no
        // longer matches what is on disk.
        ws.write("vendor/skills/python.md", "# python\n");
        expect(await runOmni(["projection", "sync"], { cwd: ws.cwd })).toHaveSucceeded();

        const stale = await runOmni(["ignore", "sync", "--check"], {
            cwd: ws.cwd,
        });
        expect(stale).toHaveFailed();
    });

    it("clean removes only the block, leaving surrounding lines intact", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { ".gitignore": "node_modules\ndist\n" },
        });

        expect(await runOmni(["ignore", "sync"], { cwd: ws.cwd })).toHaveSucceeded();
        expect(ws.read(".gitignore")).toContain(BEGIN);

        expect(await runOmni(["ignore", "clean"], { cwd: ws.cwd })).toHaveSucceeded();
        const cleaned = ws.read(".gitignore");
        expect(cleaned).not.toContain(BEGIN);
        expect(cleaned).not.toContain(END);
        expect(cleaned).toContain("node_modules");
        expect(cleaned).toContain("dist");
    });

    it("honors a configured non-default files list", async () => {
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                ignore: { files: ["custom.ignore"] },
            },
        });

        expect(await runOmni(["ignore", "sync"], { cwd: ws.cwd })).toHaveSucceeded();
        expect(ws.read("custom.ignore")).toContain(BEGIN);
        expect(ws.exists(".gitignore")).toBe(false);
        expect(ws.exists(".ignore")).toBe(false);
    });

    it("preserves each seeded file's own line ending", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".gitignore": "node_modules\r\ndist\r\n",
                ".ignore": "cache\n",
            },
        });

        expect(await runOmni(["ignore", "sync"], { cwd: ws.cwd })).toHaveSucceeded();

        const crlf = ws.read(".gitignore");
        expect(crlf).toContain(BEGIN);
        expect(crlf).toContain("\r\n");
        // No bare LF that is not part of a CRLF pair.
        expect(/[^\r]\n/.test(crlf)).toBe(false);

        const lf = ws.read(".ignore");
        expect(lf).toContain(BEGIN);
        expect(lf).not.toContain("\r");
    });
});
