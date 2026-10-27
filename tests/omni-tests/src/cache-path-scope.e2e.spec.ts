import { mkdirSync, rmSync, symlinkSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { makeWorkspace, runOmni, type Workspace } from "@/harness";

interface PathCase {
    name: string;
    pattern: (ws: Workspace, file: string) => string;
    setup?: (ws: Workspace) => void;
}

const siblingCases: PathCase[] = [
    { name: "parent-relative file", pattern: (_, file) => `../shared/${file}` },
    { name: "parent-relative glob", pattern: () => "../shared/*.txt" },
    {
        name: "project-rooted traversal",
        pattern: () => "@project/../shared/*.txt",
    },
    { name: "workspace-rooted glob", pattern: () => "@workspace/shared/*.txt" },
    { name: "absolute file", pattern: (ws, file) => ws.path("shared", file) },
    { name: "absolute glob", pattern: (ws) => ws.path("shared", "*.txt") },
    {
        name: "symlink to a sibling directory",
        pattern: () => "linked/*.txt",
        setup: (ws) => {
            mkdirSync(ws.path("shared"), { recursive: true });
            symlinkSync(
                ws.path("shared"),
                ws.path("app", "linked"),
                process.platform === "win32" ? "junction" : "dir",
            );
        },
    },
];

const externalCases: PathCase[] = [
    {
        name: "parent-relative file",
        pattern: () => "../../external/artifact.txt",
    },
    { name: "parent-relative glob", pattern: () => "../../external/*.txt" },
    {
        name: "project-rooted traversal",
        pattern: () => "@project/../../external/*.txt",
    },
    {
        name: "workspace-rooted traversal",
        pattern: () => "@workspace/../external/*.txt",
    },
    {
        name: "absolute file",
        pattern: (ws) => ws.path("external/artifact.txt"),
    },
    { name: "absolute glob", pattern: (ws) => ws.path("external/*.txt") },
];

const artifactWriter = [
    'const fs = require("node:fs");',
    'const path = require("node:path");',
    "const target = process.argv[2];",
    "fs.mkdirSync(path.dirname(target), { recursive: true });",
    'fs.writeFileSync(target, require("node:crypto").randomUUID());',
    'console.log("wrote fresh artifact");',
].join("\n");

async function projectHash(cwd: string): Promise<string> {
    const result = await runOmni(
        ["hash", "-r", "project", "app", "-t", "build"],
        { cwd },
    );
    expect(result).toHaveSucceeded();
    expect(result.stdout).toMatch(/^[A-Za-z0-9]{40,}$/);
    return result.stdout;
}

describe("cache paths outside the project", { tags: ["caching"] }, () => {
    it.each(siblingCases)(
        "$name participates in the input hash and invalidates a cached task",
        async ({ pattern, setup }) => {
            const ws = makeWorkspace({
                projects: {
                    app: { name: "app", tasks: { build: "node build.cjs" } },
                },
                files: {
                    "app/build.cjs": 'console.log("executed build");',
                    "shared/input.txt": "original input",
                },
            });
            setup?.(ws);
            ws.write("app/project.omni.yaml", {
                name: "app",
                cache: {
                    key: {
                        defaults: false,
                        files: ["build.cjs", pattern(ws, "input.txt")],
                    },
                },
                tasks: { build: "node build.cjs" },
            });

            const before = await projectHash(ws.cwd);
            const first = await runOmni(["run", "build"], { cwd: ws.cwd });
            expect(first).toHaveSucceeded();
            expect(first).toOutputContaining("Executed task 'app#build'");

            const cached = await runOmni(["run", "build"], { cwd: ws.cwd });
            expect(cached).toHaveSucceeded();
            expect(cached).toOutputContaining("Cache hit for task 'app#build'");

            ws.write(
                "shared/input.txt",
                "edited sibling input with a different size",
            );
            expect(await projectHash(ws.cwd)).not.toBe(before);
            const changed = await runOmni(["run", "build"], { cwd: ws.cwd });
            expect(changed).toHaveSucceeded();
            expect(changed).toOutputContaining("Executed task 'app#build'");
            expect(changed.stdout).not.toContain("Cache hit for task");
        },
    );

    it.each(siblingCases)(
        "$name restores a sibling output on a cache hit",
        async ({ pattern, setup }) => {
            const ws = makeWorkspace({
                projects: {
                    app: {
                        name: "app",
                        tasks: {
                            build: "node build.cjs ../shared/artifact.txt",
                        },
                    },
                },
                files: { "app/build.cjs": artifactWriter },
            });
            setup?.(ws);
            ws.write("app/project.omni.yaml", {
                name: "app",
                cache: {
                    key: { defaults: false, files: ["build.cjs"] },
                    output: { files: [pattern(ws, "artifact.txt")] },
                },
                tasks: { build: "node build.cjs ../shared/artifact.txt" },
            });

            const first = await runOmni(["run", "build"], { cwd: ws.cwd });
            expect(first).toHaveSucceeded();
            const original = ws.read("shared/artifact.txt");
            expect(original).not.toBe("");
            rmSync(ws.path("shared/artifact.txt"));
            expect(ws.exists("shared/artifact.txt")).toBe(false);

            const cached = await runOmni(["run", "build"], { cwd: ws.cwd });
            expect(cached).toHaveSucceeded();
            expect(cached).toOutputContaining("Cache hit for task 'app#build'");
            // A rerun writes a new UUID, so equal bytes prove actual restoration.
            expect(ws.read("shared/artifact.txt")).toBe(original);
        },
    );

    it.each(externalCases)(
        "$name outside the workspace is neither hashed nor restored",
        async ({ pattern }) => {
            // Nest the real workspace inside the disposable fixture so external
            // files also stay under harness-owned cleanup.
            const ws = makeWorkspace({
                workspace: { projects: [] },
                files: {
                    "workspace/workspace.omni.yaml": "projects: ['app/**']\n",
                    "workspace/app/build.cjs": artifactWriter,
                    "external/artifact.txt": "original external input",
                },
            });
            const cwd = ws.path("workspace");
            const external = pattern(ws, "artifact.txt");
            ws.write("workspace/app/project.omni.yaml", {
                name: "app",
                cache: {
                    key: { defaults: false, files: ["build.cjs", external] },
                    output: { files: [external] },
                },
                tasks: { build: "node build.cjs ../../external/artifact.txt" },
            });

            const before = await projectHash(cwd);
            ws.write(
                "external/artifact.txt",
                "edited external input with a different size",
            );
            expect(await projectHash(cwd)).toBe(before);

            const first = await runOmni(["run", "build"], { cwd });
            expect(first).toHaveSucceeded();
            expect(ws.read("external/artifact.txt")).not.toBe("");
            rmSync(ws.path("external/artifact.txt"));

            const cached = await runOmni(["run", "build"], { cwd });
            expect(cached).toHaveSucceeded();
            expect(cached).toOutputContaining("Cache hit for task 'app#build'");
            expect(ws.exists("external/artifact.txt")).toBe(false);

            ws.write(
                "workspace/app/build.cjs",
                `${artifactWriter}\n// changed local input\n`,
            );
            expect(await projectHash(cwd)).not.toBe(before);
        },
    );
});
