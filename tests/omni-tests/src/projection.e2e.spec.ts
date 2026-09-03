/**
 * `omni projection` - workspace projections end to end: a local source whose
 * files are materialized into a workspace destination via links, tracked in the
 * `.omni/sources/projection/links.json` ledger for idempotent sync and safe
 * teardown. Pinned to `crates/omni_projections/*`,
 * `crates/omni_api/src/operations/projection.rs`, and
 * `crates/omni_cli_core/src/commands/projection.rs`.
 */

import { readdirSync, rmSync, writeFileSync } from "node:fs";
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
                    routes: [
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

// A mirror projection that opts back into the pre-error move-aside behavior
// with `on_existing: backup`.
function backupMirrorWorkspace(): WorkspaceSpec {
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
                            on_existing: "backup",
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

// Two explicit rules that point different source files at one destination, so
// the plan contradicts itself: an always-fatal collision no policy can permit.
function collidingRulesWorkspace(): WorkspaceSpec {
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
                            strategy: "explicit",
                            target: "@workspace/out",
                            rules: [
                                { source: "a.md", dest: "@target/same.md" },
                                { source: "b.md", dest: "@target/same.md" },
                            ],
                        },
                    ],
                },
            ],
        },
        files: {
            "vendor/skills/a.md": "# a\n",
            "vendor/skills/b.md": "# b\n",
        },
    };
}

function ledgerLinkCount(ws: ReturnType<typeof makeWorkspace>): number {
    const ledger = JSON.parse(
        ws.read(".omni/sources/projection/links.json"),
    ) as { links: unknown[] };
    return ledger.links.length;
}

// A `pattern` projection with `match_kind: dir` links whole directories: one
// directory link per matched folder, rather than a link per file.
function dirRoutingWorkspace(): WorkspaceSpec {
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
                            strategy: "pattern",
                            target: "@workspace/.agents/skills",
                            rules: [
                                {
                                    match: "engineering/*",
                                    match_kind: "dir",
                                    dest: "@target/{basename}",
                                },
                            ],
                        },
                    ],
                },
            ],
        },
        files: {
            "vendor/skills/engineering/tdd/SKILL.md": "# tdd\n",
            "vendor/skills/engineering/code-review/SKILL.md": "# review\n",
        },
    };
}

// A `pattern` route whose `match` is a list: entries are the OR-union of the
// include globs, and a leading `!` marks an exclusion that always wins.
function listMatchWorkspace(): WorkspaceSpec {
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
                            strategy: "pattern",
                            target: "@workspace/.agents/skills",
                            rules: [
                                {
                                    match: {
                                        include: ["**/*.md"],
                                        exclude: ["drafts/**"],
                                    },
                                    dest: "@target/{name}.md",
                                },
                            ],
                        },
                    ],
                },
            ],
        },
        files: {
            "vendor/skills/rust.md": "# rust\n",
            "vendor/skills/go.md": "# go\n",
            "vendor/skills/drafts/wip.md": "# wip\n",
        },
    };
}

// A `mirror` route whose `scope` uses the include/exclude object form: mirror
// everything an include matches except what `exclude` drops, preserving source
// structure.
function listScopeWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            projections: [
                {
                    source: "local",
                    path: "./vendor/docs",
                    id: "local-docs",
                    routes: [
                        {
                            strategy: "mirror",
                            target: "@workspace/.agents/docs",
                            scope: {
                                include: ["docs/**"],
                                exclude: ["docs/drafts/**"],
                            },
                        },
                    ],
                },
            ],
        },
        files: {
            "vendor/docs/docs/guide.md": "# guide\n",
            "vendor/docs/docs/drafts/wip.md": "# wip\n",
            "vendor/docs/README.md": "# readme\n",
        },
    };
}

// A source that ships its own `projection.omni.yaml`; the workspace omits
// `routes`, so the owned manifest is inherited.
function ownedProjectionWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            projections: [
                {
                    source: "local",
                    path: "./vendor/skills",
                    id: "local-skills",
                },
            ],
        },
        files: {
            "vendor/skills/rust.md": "# rust\n",
            "vendor/skills/projection.omni.yaml":
                'routes:\n  - strategy: mirror\n    scope: "*.md"\n    target: "@workspace/.agents/skills"\n',
        },
    };
}

// Two independent local sources mirroring into distinct destinations, used to
// exercise reconciliation when one is later dropped from config.
function twoSourceWorkspace(): WorkspaceSpec {
    return {
        workspace: {
            projects: ["**"],
            projections: [
                {
                    source: "local",
                    path: "./vendor/a",
                    id: "skills-a",
                    routes: [
                        { strategy: "mirror", target: "@workspace/.agents/a" },
                    ],
                },
                {
                    source: "local",
                    path: "./vendor/b",
                    id: "skills-b",
                    routes: [
                        { strategy: "mirror", target: "@workspace/.agents/b" },
                    ],
                },
            ],
        },
        files: {
            "vendor/a/one.md": "# one\n",
            "vendor/b/two.md": "# two\n",
        },
    };
}

// The same workspace with `skills-b` removed from config.
const singleSourceConfig = {
    projects: ["**"],
    projections: [
        {
            source: "local",
            path: "./vendor/a",
            id: "skills-a",
            routes: [{ strategy: "mirror", target: "@workspace/.agents/a" }],
        },
    ],
};

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

    it("links whole directories with pattern match_kind: dir", async () => {
        const ws = makeWorkspace(dirRoutingWorkspace());

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();

        // One directory link per matched folder; contents resolve through it.
        expect(ws.read(".agents/skills/tdd/SKILL.md")).toBe("# tdd\n");
        expect(ws.read(".agents/skills/code-review/SKILL.md")).toBe(
            "# review\n",
        );
        // Two directory links, not four file links.
        expect(ledgerLinkCount(ws)).toBe(2);

        const status = await runOmni(["projection", "status"], { cwd: ws.cwd });
        expect(status.stdout).toContain("2 ok");

        // An unchanged re-sync is a no-op.
        const second = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(second).toHaveSucceeded();
        expect(second.stdout).toContain("up-to-date");
    });

    it("prints planned directory-link dests on --dry-run without writing", async () => {
        const ws = makeWorkspace(dirRoutingWorkspace());

        const result = await runOmni(["projection", "sync", "--dry-run"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain(".agents/skills/tdd");
        expect(result.stdout).toContain(".agents/skills/code-review");
        expect(ws.exists(".agents/skills/tdd/SKILL.md")).toBe(false);
    });

    it("routes an include/exclude match as an OR-union with excludes", async () => {
        const ws = makeWorkspace(listMatchWorkspace());

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();

        // Both includes match; the `drafts/**` exclude wins over `**/*.md`.
        expect(ws.read(".agents/skills/rust.md")).toBe("# rust\n");
        expect(ws.read(".agents/skills/go.md")).toBe("# go\n");
        expect(ws.exists(".agents/skills/wip.md")).toBe(false);
        // Two links, not three: the excluded draft never produces one.
        expect(ledgerLinkCount(ws)).toBe(2);
    });

    it("narrows a mirror with an include/exclude scope", async () => {
        const ws = makeWorkspace(listScopeWorkspace());

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();

        // `docs/**` includes the guide; `docs/drafts/**` excludes the draft;
        // `README.md` is outside every include, so it is not mirrored.
        expect(ws.read(".agents/docs/docs/guide.md")).toBe("# guide\n");
        expect(ws.exists(".agents/docs/docs/drafts/wip.md")).toBe(false);
        expect(ws.exists(".agents/docs/README.md")).toBe(false);
        expect(ledgerLinkCount(ws)).toBe(1);
    });

    it("inherits routes from a source's own projection.omni.yaml", async () => {
        const ws = makeWorkspace(ownedProjectionWorkspace());

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();
        expect(ws.read(".agents/skills/rust.md")).toBe("# rust\n");
    });

    it("errors when a source declares no routes anywhere", async () => {
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                projections: [
                    {
                        source: "local",
                        path: "./vendor/skills",
                        id: "local-skills",
                    },
                ],
            },
            files: { "vendor/skills/rust.md": "# rust\n" },
        });

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveFailed();
    });

    it("errors when routes is an explicit empty list", async () => {
        const ws = makeWorkspace({
            workspace: {
                projects: ["**"],
                projections: [
                    {
                        source: "local",
                        path: "./vendor/skills",
                        id: "local-skills",
                        routes: [],
                    },
                ],
            },
            files: { "vendor/skills/rust.md": "# rust\n" },
        });

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveFailed();
    });

    it("reconciles a source removed from config on a full sync", async () => {
        const ws = makeWorkspace(twoSourceWorkspace());

        await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(ws.exists(".agents/a/one.md")).toBe(true);
        expect(ws.exists(".agents/b/two.md")).toBe(true);
        expect(ledgerLinkCount(ws)).toBe(2);

        // Drop `skills-b` from config, then run a full sync.
        ws.write("workspace.omni.yaml", singleSourceConfig);
        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain("removed");

        // The dropped source's links are gone; the survivor is untouched.
        expect(ws.exists(".agents/b/two.md")).toBe(false);
        expect(ws.read(".agents/a/one.md")).toBe("# one\n");
        expect(ledgerLinkCount(ws)).toBe(1);

        const status = await runOmni(["projection", "status"], { cwd: ws.cwd });
        expect(status.stdout).toContain("1 ok");
    });

    it("reports orphaned links on --dry-run without removing them", async () => {
        const ws = makeWorkspace(twoSourceWorkspace());

        await runOmni(["projection", "sync"], { cwd: ws.cwd });

        ws.write("workspace.omni.yaml", singleSourceConfig);
        const result = await runOmni(["projection", "sync", "--dry-run"], {
            cwd: ws.cwd,
        });
        expect(result).toHaveSucceeded();

        // The orphan is still on disk and still recorded after a dry run.
        expect(ws.exists(".agents/b/two.md")).toBe(true);
        expect(ledgerLinkCount(ws)).toBe(2);
    });

    it("leaves orphans untouched on a filtered sync until the next full sync", async () => {
        const ws = makeWorkspace(twoSourceWorkspace());

        await runOmni(["projection", "sync"], { cwd: ws.cwd });
        ws.write("workspace.omni.yaml", singleSourceConfig);

        // A targeted run touches only `skills-a`, leaving the orphan in place.
        const filtered = await runOmni(
            ["projection", "sync", "--source", "skills-a"],
            { cwd: ws.cwd },
        );
        expect(filtered).toHaveSucceeded();
        expect(ws.exists(".agents/b/two.md")).toBe(true);

        // A later full sync reconciles it.
        const full = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(full).toHaveSucceeded();
        expect(ws.exists(".agents/b/two.md")).toBe(false);
    });

    it("restores a displaced file with unlink --restore-backups", async () => {
        const ws = makeWorkspace(backupMirrorWorkspace());

        // With `on_existing: backup`, a pre-existing local file at the
        // destination is moved aside on sync and the backup is recorded.
        ws.write(".agents/skills/rust.md", "my own notes\n");
        await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(ws.read(".agents/skills/rust.md")).toBe("# rust\n");

        const result = await runOmni(
            ["projection", "unlink", "local-skills", "--restore-backups"],
            { cwd: ws.cwd },
        );
        expect(result).toHaveSucceeded();
        expect(result.stdout).toContain("restored");

        // The displaced file is back with its original content.
        expect(ws.read(".agents/skills/rust.md")).toBe("my own notes\n");
    });

    it("rejects unlink with both --clean-backups and --restore-backups", async () => {
        const ws = makeWorkspace(projectionWorkspace());
        await runOmni(["projection", "sync"], { cwd: ws.cwd });

        const result = await runOmni(
            [
                "projection",
                "unlink",
                "local-skills",
                "--clean-backups",
                "--restore-backups",
            ],
            { cwd: ws.cwd },
        );
        expect(result).toHaveFailed();
    });

    it("fails the sync when a foreign file sits at a destination", async () => {
        const ws = makeWorkspace(projectionWorkspace());
        // A file omni does not own already occupies a destination.
        ws.write(".agents/skills/rust.md", "FOREIGN");

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("already exists");

        // The foreign file is untouched and the whole run wrote nothing.
        expect(ws.read(".agents/skills/rust.md")).toBe("FOREIGN");
        expect(ws.exists(".agents/skills/sub/python.md")).toBe(false);
        expect(ws.exists(".omni/sources/projection/links.json")).toBe(false);
    });

    it("moves a foreign file aside with on_existing: backup", async () => {
        const ws = makeWorkspace(backupMirrorWorkspace());
        ws.write(".agents/skills/rust.md", "FOREIGN");

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveSucceeded();

        // The link now resolves to the source, and the displaced file is kept
        // as a timestamped backup sibling.
        expect(ws.read(".agents/skills/rust.md")).toBe("# rust\n");
        const siblings = readdirSync(ws.path(".agents/skills"));
        const backup = siblings.find((name) => name.startsWith("rust.md.bak."));
        expect(backup).toBeDefined();
        expect(ws.read(`.agents/skills/${backup}`)).toBe("FOREIGN");
    });

    it("aborts when two rules resolve to one destination", async () => {
        const ws = makeWorkspace(collidingRulesWorkspace());

        const result = await runOmni(["projection", "sync"], { cwd: ws.cwd });
        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining("collision");

        // A self-contradictory plan writes nothing.
        expect(ws.exists("out/same.md")).toBe(false);
        expect(ws.exists(".omni/sources/projection/links.json")).toBe(false);
    });
});
