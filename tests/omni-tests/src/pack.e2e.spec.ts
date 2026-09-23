/**
 * `omni pack` - source packs end to end: one `packs:` entry bundles generators,
 * tools, and projections behind a single `pack.omni.*` manifest and registers
 * into all three subsystems at once. Read-only `omni pack list|tree|info`
 * inspect the expanded graph. Pinned to `crates/omni_configurations/src/pack.rs`,
 * `crates/omni_remote_source_contributors/src/pack.rs`,
 * `crates/omni_api/src/operations/pack.rs`, and
 * `crates/omni_cli_core/src/commands/pack.rs`.
 */

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type WorkspaceSpec } from "@/harness";

// A workspace with one local pack that ships a projection face.
function projectionPackWorkspace(provides?: string[]): WorkspaceSpec {
    const entry: Record<string, unknown> = {
        source: "local",
        path: "./packs/kit",
        id: "kit",
    };
    if (provides) {
        entry.provides = provides;
    }
    return {
        workspace: { projects: ["**"], packs: [entry] },
        files: {
            "packs/kit/pack.omni.yaml": [
                'name: "@org/kit"',
                "version: 1.2.3",
                "description: A test pack",
                "projections:",
                "  - source: local",
                "    path: ./skills",
                "    id: skills",
                "    routes:",
                "      - strategy: mirror",
                '        target: "@workspace/.agents/skills"',
                "",
            ].join("\n"),
            "packs/kit/skills/rust.md": "# rust\n",
        },
    };
}

// A pack that composes a sub-pack (pack-of-packs).
function nestedPackWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            packs: [{ source: "local", path: "./packs/a", id: "a" }],
        },
        files: {
            "packs/a/pack.omni.yaml": [
                'name: "@org/a"',
                "generators:",
                "  - source: local",
                "    path: ./gen",
                "packs:",
                "  - source: local",
                "    path: ./b",
                "    id: b",
                "",
            ].join("\n"),
            "packs/a/gen/keep.txt": "gen dir\n",
            "packs/a/b/pack.omni.yaml": [
                'name: "@org/b"',
                "tools:",
                "  - source: local",
                "    path: ./tools",
                "",
            ].join("\n"),
            "packs/a/b/tools/keep.txt": "tools dir\n",
        },
    };
}

describe("+pack @e2e", { tags: ["pack"] }, () => {
    it("lists the top-level packs with id, name, and version", async () => {
        const ws = makeWorkspace(projectionPackWorkspace());

        const result = await runOmni(["pack", "list"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain("kit");
        expect(result.stdout).toContain("@org/kit");
        expect(result.stdout).toContain("1.2.3");
        expect(result.stdout).toContain("projections");
    });

    it("prints the pack-of-packs graph with tree", async () => {
        const ws = makeWorkspace(nestedPackWorkspace());

        const result = await runOmni(["pack", "tree"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();
        // The parent and its qualified sub-pack are both present.
        expect(result.stdout).toContain("a");
        expect(result.stdout).toContain("a::b");
        expect(result.stdout).toContain("@org/b");
    });

    it("shows a single pack subtree with info", async () => {
        const ws = makeWorkspace(nestedPackWorkspace());

        const result = await runOmni(["pack", "info", "a::b"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain("a::b");
        expect(result.stdout).toContain("tools");
    });

    it("materializes a pack-contributed projection on sync", async () => {
        const ws = makeWorkspace(projectionPackWorkspace());

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();
        expect(ws.read(".agents/skills/rust.md")).toBe("# rust\n");
    });

    it("honors a generators-only provides gate and skips the projection face", async () => {
        const ws = makeWorkspace(projectionPackWorkspace(["generators"]));

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();
        // provides narrows to generators, so the projection is never linked.
        expect(ws.exists(".agents/skills/rust.md")).toBe(false);
    });

    it("errors when a packs entry has no pack.omni.* manifest", async () => {
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                packs: [
                    { source: "local", path: "./packs/empty", id: "empty" },
                ],
            },
            files: { "packs/empty/keep.txt": "no manifest here\n" },
        });

        const result = await runOmni(["pack", "list"], { cwd: ws.cwd });
        expect(result).toHaveFailed();
    });
});
