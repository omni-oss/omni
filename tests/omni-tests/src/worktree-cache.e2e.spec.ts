import { readFileSync, statSync, utimesSync } from "node:fs";
import { join } from "node:path";

import { execa } from "execa";
import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, skipUnlessGitCliAvailable } from "@/harness";
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
    const result = await runOmni(
        ["--stdout-log-level", "error", ...args, "cache", "dir"],
        { cwd },
    );
    expect(result).toHaveSucceeded();
    return cleanPath(result.out);
}

describe("automatic Git worktree cache sharing", { tags: ["caching"] }, () => {
    it("restores shared artifacts locally and isolates equal-mtime hash indexes", async (ctx) => {
        await skipUnlessGitCliAvailable(ctx);
        const { ws, primary, linked, git } = await fixture();
        expect(await cacheDir(linked)).toBe(await cacheDir(primary));
        const first = await runOmni(["run", "build"], { cwd: primary });
        expect(first).toHaveSucceeded();
        expect(first).toOutputContaining("Executed task 'app#build'");
        const artifact = ws.read("primary/app/artifact.txt");
        const cached = await runOmni(["run", "build"], { cwd: linked });
        expect(cached).toHaveSucceeded();
        expect(cached).toOutputContaining("Cache hit for task 'app#build'");
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
        expect(changed).toOutputContaining("Executed task 'app#build'");
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
