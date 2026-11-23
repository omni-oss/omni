import {
    existsSync,
    readFileSync,
    rmSync,
    statSync,
    utimesSync,
} from "node:fs";
import { join } from "node:path";

import { execa } from "execa";
import { describe, expect, it } from "vitest";

import {
    makeWorkspace,
    resolveOmniBin,
    runOmni,
    skipUnlessGitCliAvailable,
} from "@/harness";
import { cleanPath } from "@/utils";

async function fixture() {
    const ws = makeWorkspace({
        files: {
            "primary/workspace.omni.yaml": "projects: ['app/**']\n",
            "primary/app/project.omni.yaml": [
                "name: app",
                "cache:",
                "  key: { defaults: false, files: ['build.cjs', 'input.txt'] }",
                "  output: { files: ['artifact.txt'] }",
                "tasks: { build: 'node build.cjs' }",
            ].join("\n"),
            "primary/app/input.txt": "aaaa",
            "primary/app/build.cjs": [
                'const fs = require("node:fs");',
                'fs.writeFileSync("artifact.txt", fs.readFileSync("input.txt", "utf8") + require("node:crypto").randomUUID());',
                'console.log("executed fresh build");',
            ].join("\n"),
        },
    });
    const primary = ws.path("primary");
    const linked = ws.path("linked");
    const git = (args: string[]) =>
        execa("git", ["-c", "commit.gpgsign=false", ...args], {
            cwd: primary,
            env: {
                GIT_CONFIG_NOSYSTEM: "1",
                GIT_CONFIG_GLOBAL: "/dev/null",
                GIT_AUTHOR_NAME: "test",
                GIT_AUTHOR_EMAIL: "test@example.com",
                GIT_COMMITTER_NAME: "test",
                GIT_COMMITTER_EMAIL: "test@example.com",
            },
        });
    await git(["init"]);
    await git(["add", "."]);
    await git(["commit", "-m", "fixture"]);
    await git(["worktree", "add", "--detach", linked]);
    return { ws, primary, linked, git };
}

async function cacheDir(cwd: string, args: string[] = []) {
    const result = await runOmni([...args, "cache", "dir"], { cwd });
    expect(result).toHaveSucceeded();
    return cleanPath(result.out);
}

describe("automatic Git worktree cache sharing", { tags: ["caching"] }, () => {
    it("keeps both worktrees' current entries in a custom cache and prunes an old digest", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked } = await fixture();
        ws.write("primary/worktree.omni.yaml", {
            cache: { dir: "../shared/cache" },
        });
        const run = (cwd: string, ...args: string[]) => runOmni(args, { cwd });
        expect(await run(primary, "run", "build")).toHaveSucceeded();
        ws.write("primary/app/input.txt", "new primary input");
        ws.write("linked/app/input.txt", "different linked input");
        expect(await run(primary, "run", "build")).toHaveSucceeded();
        expect(await run(linked, "run", "build")).toHaveSucceeded();

        const prune = await run(linked, "cache", "prune", "--stale", "--yes");
        expect(prune).toHaveSucceeded();
        expect(prune).toOutputContaining("Pruned 1 cache entries");
        for (const cwd of [primary, linked]) {
            expect(await run(cwd, "run", "build")).toHaveStderrContaining(
                "Cache hit for task 'app#build'",
            );
        }
    });

    it("preserves a peer's disabled task and refuses deletion after a peer load failure", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked } = await fixture();
        const run = (cwd: string, ...args: string[]) => runOmni(args, { cwd });
        expect(await run(primary, "run", "build")).toHaveSucceeded();
        ws.write("primary/app/project.omni.yaml", { name: "app" });
        ws.write("linked/app/project.omni.yaml", {
            name: "app",
            tasks: { build: { enabled: false } },
        });
        const pruneArgs = ["cache", "prune", "--orphan", "--task", "build"];
        const live = await run(primary, ...pruneArgs, "--dry-run");
        expect(live).toHaveSucceeded();
        expect(live).toOutputContaining(
            "No cache entries matched the given filters",
        );

        ws.write("linked/app/project.omni.yaml", "name: [");
        const failed = await run(primary, ...pruneArgs, "--yes");
        expect(failed.failed).toBe(true);
        expect(failed.stderr).toContain("cannot load peer workspace");

        ws.write("linked/worktree.omni.yaml", {
            cache: { dir: "private-cache" },
        });
        const separateCache = await run(primary, ...pruneArgs, "--dry-run");
        expect(separateCache).toHaveSucceeded();
        expect(separateCache).toOutputContaining("would prune 1 cache entries");
        rmSync(join(linked, "worktree.omni.yaml"));

        ws.write("linked/app/project.omni.yaml", { name: "app" });
        const preview = await run(primary, ...pruneArgs, "--dry-run");
        expect(preview).toHaveSucceeded();
        expect(preview).toOutputContaining("would prune 1 cache entries");
        const prune = await run(primary, ...pruneArgs, "--yes");
        expect(prune).toHaveSucceeded();
        expect(prune).toOutputContaining("Pruned 1 cache entries");
    });

    it("stops when a peer has no workspace and the cache path is configured", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked } = await fixture();
        ws.write("primary/worktree.omni.yaml", {
            cache: { dir: "../shared/cache" },
        });
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveSucceeded();
        rmSync(join(linked, "workspace.omni.yaml"));
        const result = await runOmni(["cache", "prune", "--orphan", "--yes"], {
            cwd: primary,
        });
        expect(result.failed).toBe(true);
        expect(result.stderr).toContain("cannot tell whether the worktree");
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveStderrContaining("Cache hit for task 'app#build'");
    });

    it("does not let an unrelated task with an unevaluable template stop a filtered stale prune", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary } = await fixture();
        for (const checkout of ["primary", "linked"]) {
            ws.write(`${checkout}/workspace.omni.yaml`, {
                projects: ["app/**", "other/**"],
            });
            ws.write(`${checkout}/other/project.omni.yaml`, {
                name: "other",
                tasks: {
                    t: { exec: "node -e 0", enabled: "{{ args.missing }}" },
                },
            });
        }
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveSucceeded();
        const stale = await runOmni(
            ["cache", "prune", "--stale", "--dir", "app/**", "--yes"],
            { cwd: primary },
        );
        expect(stale).toHaveSucceeded();
        expect(stale).toOutputContaining(
            "No cache entries matched the given filters",
        );
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveStderrContaining("Cache hit for task 'app#build'");
    });

    it("rejects conflicting or retired flags before loading a workspace", async () => {
        const ws = makeWorkspace({ files: {} });
        const conflict = await runOmni(
            ["cache", "prune", "--stale", "--orphan"],
            { cwd: ws.cwd },
        );
        expect(conflict.failed).toBe(true);
        expect(conflict.stderr).toContain("cannot be used with");
        const retired = await runOmni(["cache", "prune", "--stale-only"], {
            cwd: ws.cwd,
        });
        expect(retired.failed).toBe(true);
        expect(retired.stderr).toContain("unexpected argument '--stale-only'");
    });

    it("rechecks the peer task inventory after interactive confirmation", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked } = await fixture();
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveSucceeded();
        const peerConfig = ws.read("linked/app/project.omni.yaml");
        ws.write("primary/app/project.omni.yaml", { name: "app" });
        ws.write("linked/app/project.omni.yaml", { name: "app" });

        const child = execa(resolveOmniBin(), ["cache", "prune", "--orphan"], {
            cwd: primary,
            stdin: "pipe",
            timeout: 10_000,
        });
        let preview = "";
        child.stdout?.on("data", (chunk: Buffer) => {
            preview += chunk.toString();
            if (preview.includes("[y/N]") && !child.stdin?.writableEnded) {
                ws.write("linked/app/project.omni.yaml", peerConfig);
                child.stdin?.end("y\n");
            }
        });
        const result = await child;
        expect(preview).toContain("Project: app");
        expect(result.stdout).toContain("Pruned 0 cache entries");
        expect(
            await runOmni(["run", "build"], { cwd: linked }),
        ).toHaveStderrContaining("Cache hit for task 'app#build'");
    });

    it("keeps a digest that uses a commandless dependency's forwarded hash", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked } = await fixture();
        const project = {
            name: "app",
            cache: { key: { defaults: false, files: ["input.txt"] } },
            tasks: {
                source: "node -e \"console.log('source')\"",
                aggregate: { dependencies: ["source"] },
                build: { exec: "node build.cjs", dependencies: ["aggregate"] },
            },
        };
        ws.write("primary/app/project.omni.yaml", project);
        ws.write("linked/app/project.omni.yaml", project);
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveSucceeded();
        const stale = await runOmni(
            ["cache", "prune", "--stale", "--task", "build", "--dry-run"],
            { cwd: linked },
        );
        expect(stale).toHaveSucceeded();
        expect(stale).toOutputContaining(
            "No cache entries matched the given filters",
        );
    });

    it("keeps templated cache paths current when a project glob matches only one worktree", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked } = await fixture();
        ws.write("primary/app/project.omni.yaml", {
            name: "app-web",
            cache: {
                key: { defaults: false, files: ["input.txt"] },
                output: { files: ["{{ args.artifact }}"] },
            },
            tasks: {
                build: {
                    exec: "node build.cjs",
                    args: { artifact: "artifact.txt" },
                },
            },
        });
        ws.write("linked/app/project.omni.yaml", { name: "other" });
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveSucceeded();
        const prune = await runOmni(
            [
                "cache",
                "prune",
                "--stale",
                "--project-pattern",
                "app-*",
                "--yes",
            ],
            { cwd: primary },
        );
        expect(prune).toHaveSucceeded();
        expect(prune).toOutputContaining(
            "No cache entries matched the given filters",
        );
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveStderrContaining("Cache hit for task 'app-web#build'");
        const invalid = await runOmni(
            [
                "cache",
                "prune",
                "--stale",
                "--project-pattern",
                "[",
                "--dry-run",
            ],
            { cwd: linked },
        );
        expect(invalid.failed).toBe(true);
    });

    it("keeps an entry when a dependency makes the task uncacheable", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary } = await fixture();
        const project = (cacheEnabled: boolean) => ({
            name: "app",
            cache: { key: { defaults: false, files: ["input.txt"] } },
            tasks: {
                source: {
                    exec: "node -e \"console.log('source')\"",
                    cache: { enabled: cacheEnabled },
                },
                aggregate: { dependencies: ["source"] },
                build: { exec: "node build.cjs", dependencies: ["aggregate"] },
            },
        });
        ws.write("primary/app/project.omni.yaml", project(true));
        ws.write("linked/app/project.omni.yaml", project(true));
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveSucceeded();
        ws.write("primary/app/project.omni.yaml", project(false));
        ws.write("linked/app/project.omni.yaml", project(false));
        const prune = await runOmni(
            ["cache", "prune", "--stale", "--task", "build", "--yes"],
            { cwd: primary },
        );
        expect(prune).toHaveSucceeded();
        expect(prune).toOutputContaining(
            "No cache entries matched the given filters",
        );
        ws.write("primary/app/project.omni.yaml", project(true));
        expect(
            await runOmni(["run", "build"], { cwd: primary }),
        ).toHaveStderrContaining("Cache hit for task 'app#build'");
    });

    it.for(["workspace", "dot-env", "project", "task"] as const)(
        "does not execute %s environment commands while scanning a peer",
        async (scope, ctx) => {
            await skipUnlessGitCliAvailable(ctx);
            const { ws, primary, linked } = await fixture();
            expect(
                await runOmni(["run", "build"], { cwd: primary }),
            ).toHaveSucceeded();
            const vars = { PRUNE_PROBE: "$(node prune-probe.cjs)" };
            const script =
                'require("node:fs").writeFileSync("prune-ran", "unexpected");';
            ws.write("linked/prune-probe.cjs", script);
            ws.write("linked/app/prune-probe.cjs", script);
            switch (scope) {
                case "workspace":
                    ws.write("linked/workspace.omni.yaml", {
                        projects: ["app/**"],
                        env: { vars },
                    });
                    break;
                case "dot-env":
                    ws.write(
                        "linked/.env",
                        `PRUNE_PROBE=${vars.PRUNE_PROBE}\n`,
                    );
                    break;
                case "project":
                    ws.write("linked/app/project.omni.yaml", {
                        name: "app",
                        env: { vars },
                        tasks: { build: "node build.cjs" },
                    });
                    break;
                case "task":
                    ws.write("linked/app/project.omni.yaml", {
                        name: "app",
                        tasks: {
                            build: { exec: "node build.cjs", env: { vars } },
                        },
                    });
                    break;
            }
            const orphan = await runOmni(
                ["cache", "prune", "--orphan", "--dry-run"],
                { cwd: primary },
            );
            expect(orphan).toHaveSucceeded();
            const stale = await runOmni(
                ["cache", "prune", "--stale", "--yes"],
                { cwd: primary },
            );
            expect(stale.failed).toBe(true);
            expect(stale.stderr).toContain(
                "command substitution is not allowed",
            );
            expect(existsSync(join(linked, "prune-ran"))).toBe(false);
            expect(existsSync(join(linked, "app", "prune-ran"))).toBe(false);
            expect(
                await runOmni(["run", "build"], { cwd: primary }),
            ).toHaveStderrContaining("Cache hit for task 'app#build'");
        },
    );

    it("restores shared artifacts locally and isolates equal-mtime hash indexes", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked, git } = await fixture();
        expect(await cacheDir(linked)).toBe(await cacheDir(primary));
        const first = await runOmni(["run", "build"], { cwd: primary });
        expect(first).toHaveSucceeded();
        expect(first).toHaveStderrContaining("Executed task 'app#build'");
        const artifact = ws.read("primary/app/artifact.txt");
        const cached = await runOmni(["run", "build"], { cwd: linked });
        expect(cached).toHaveSucceeded();
        expect(cached).toHaveStderrContaining("Cache hit for task 'app#build'");
        expect(ws.read("linked/app/artifact.txt")).toBe(artifact);

        // Another checkout must not reuse the primary's size/mtime file hash.
        const primaryInput = join(primary, "app/input.txt");
        const changedCheckout = ws.path("changed");
        await git(["worktree", "add", "--detach", changedCheckout]);
        const linkedInput = join(changedCheckout, "app/input.txt");
        const stamp = statSync(primaryInput);
        ws.write("changed/app/input.txt", "bbbb");
        utimesSync(linkedInput, stamp.atime, stamp.mtime);
        const changed = await runOmni(["run", "build"], {
            cwd: changedCheckout,
        });
        expect(changed).toHaveSucceeded();
        expect(changed).toHaveStderrContaining("Executed task 'app#build'");
        expect(ws.read("changed/app/artifact.txt")).toMatch(/^bbbb/);
        expect(ws.read("primary/app/artifact.txt")).toBe(artifact);
    });

    it("inherits file-relative policy and permits a local override or explicit bypass", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked } = await fixture();
        ws.write("primary/worktree.omni.yaml", {
            cache: { dir: "../shared/cache" },
        });
        expect(await cacheDir(primary)).toBe(ws.path("shared/cache"));
        expect(await cacheDir(join(linked, "app"))).toBe(
            ws.path("shared/cache"),
        );
        ws.write("linked/worktree.omni.yaml", {});
        expect(await cacheDir(linked)).toBe(ws.path("shared/cache"));
        for (const contents of ["cache: null", "cache: {dir: null}"]) {
            ws.write("linked/worktree.omni.yaml", contents);
            expect(await cacheDir(linked)).toBe(ws.path("shared/cache"));
        }
        ws.write("linked/worktree.omni.yaml", {
            cache: { dir: ".omni/cache" },
        });
        expect(await cacheDir(linked)).toBe(join(linked, ".omni/cache"));
        ws.write("linked/worktree.omni.yaml", "invalid: true");
        const invalid = await runOmni(["cache", "dir"], { cwd: linked });
        expect(invalid.failed).toBe(true);
        expect(
            await cacheDir(join(linked, "app"), [
                "--cache-dir",
                "relative-cache",
            ]),
        ).toBe(join(linked, "app/relative-cache"));
        const unsafe = await runOmni(["--cache-dir", linked, "cache", "dir"], {
            cwd: linked,
        });
        expect(unsafe.failed).toBe(true);
        expect(unsafe.stderr).toContain("unsafe cache directory");
    });

    it("synchronizes local policy ignores idempotently for every configured target", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary } = await fixture();
        const names = [".gitignore", ".ignore", ".omniignore", ".customignore"];
        ws.write("primary/workspace.omni.yaml", {
            projects: ["app/**"],
            ignore: { files: names },
        });
        const first = await runOmni(["ignore", "sync"], { cwd: primary });
        expect(first).toHaveSucceeded();
        const before = names.map((name) =>
            readFileSync(join(primary, name), "utf8"),
        );
        for (const text of before) expect(text).toContain("/worktree.omni.*");
        expect(
            await runOmni(["ignore", "sync"], { cwd: primary }),
        ).toHaveSucceeded();
        expect(
            names.map((name) => readFileSync(join(primary, name), "utf8")),
        ).toEqual(before);
    });

    it("maps nested workspaces without borrowing an unrelated parent workspace", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked } = await fixture();
        ws.write("primary/tools/docs/workspace.omni.yaml", { projects: [] });
        ws.write("linked/tools/docs/workspace.omni.yaml", { projects: [] });
        expect(await cacheDir(join(linked, "tools/docs"))).toBe(
            join(primary, "tools/docs/.omni/cache"),
        );
        ws.write("primary/tools/docs/worktree.omni.yaml", {
            cache: { dir: "state" },
        });
        expect(await cacheDir(join(linked, "tools/docs"))).toBe(
            join(primary, "tools/docs/state"),
        );
        ws.write("linked/branch-only/workspace.omni.yaml", { projects: [] });
        expect(await cacheDir(join(linked, "branch-only"))).toBe(
            join(linked, "branch-only/.omni/cache"),
        );
    });
});
