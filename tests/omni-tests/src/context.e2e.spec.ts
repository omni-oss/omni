/**
 * `+context` - workspace/root discovery, project discovery, config loading and
 * merging. These behaviors are shared by every context-needing command, so the
 * tests drive them through small, neutral commands (`env get`, `hash`, `run`).
 *
 * Pinned to `crates/omni_cli_core/src/context/*`,
 * `crates/omni_cli_core/src/configurations/*`, and the omni_configurations
 * crate. Root discovery walks up to the `workspace.omni.yaml` marker; project
 * discovery uses the workspace `projects` globs.
 */

import { rmSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
    extendsSpec,
    makeWorkspace,
    multiFormatProjectsSpec,
    runOmni,
} from "@/harness";

describe("+context @config (root & project discovery)", () => {
    it("discovers the workspace root by walking up to the marker file", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { ".env": "ROOTVAR=rootval\n", "a/b/c/.keep": "" },
        });

        // Run from a nested directory; the root `.env` must still resolve.
        const result = await runOmni(["-l", "off", "env", "get", "ROOTVAR"], {
            cwd: ws.path("a", "b", "c"),
        });

        expect(result).toHaveSucceeded();
        expect(result.stdout).toBe("rootval");
    });

    it("errors clearly when no workspace root is found", async () => {
        const ws = makeWorkspace();
        // Remove the marker so no ancestor qualifies as a workspace root.
        rmSync(ws.path("workspace.omni.yaml"));

        const result = await runOmni(["env", "all"], { cwd: ws.cwd });

        expect(result).toHaveFailed();
        expect(result).toHaveStderrContaining(
            "failed to find workspace configuration",
        );
    });

    it("discovers projects via the workspace `projects` globs", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["nested/**"] },
            projects: {
                "nested/app": { name: "in-glob", tasks: { build: "echo hi" } },
                "outside/app": {
                    name: "out-of-glob",
                    tasks: { build: "echo hi" },
                },
            },
        });

        const inGlob = await runOmni(
            ["-l", "off", "hash", "project", "in-glob"],
            {
                cwd: ws.cwd,
            },
        );
        const outOfGlob = await runOmni(
            ["-l", "off", "hash", "project", "out-of-glob"],
            { cwd: ws.cwd },
        );

        expect(inGlob).toHaveSucceeded();
        expect(outOfGlob).toHaveFailed();
        expect(outOfGlob).toHaveStderrContaining("no project found");
    });

    it("loads project config from any supported extension", async () => {
        const ws = makeWorkspace(multiFormatProjectsSpec());

        for (const name of ["yaml-app", "yml-app", "json-app", "toml-app"]) {
            const result = await runOmni(
                ["-l", "off", "hash", "project", name],
                {
                    cwd: ws.cwd,
                },
            );
            expect(result).toHaveSucceeded();
        }
    });

    it("excludes directories matched by .omniignore from discovery", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: { ".omniignore": "ignored/\n" },
            projects: {
                "ignored/app": {
                    name: "ignored-app",
                    tasks: { build: "echo hi" },
                },
                "visible/app": {
                    name: "visible-app",
                    tasks: { build: "echo hi" },
                },
            },
        });

        const ignored = await runOmni(
            ["-l", "off", "hash", "project", "ignored-app"],
            { cwd: ws.cwd },
        );
        const visible = await runOmni(
            ["-l", "off", "hash", "project", "visible-app"],
            { cwd: ws.cwd },
        );

        expect(ignored).toHaveFailed();
        expect(ignored).toHaveStderrContaining("no project found");
        expect(visible).toHaveSucceeded();
    });
});

describe("+context @config (base & extends)", () => {
    it("treats `base: true` projects as templates, not runnable projects", async () => {
        const ws = makeWorkspace(extendsSpec());

        const base = await runOmni(["-l", "off", "hash", "project", "base"], {
            cwd: ws.cwd,
        });
        const child = await runOmni(["-l", "off", "hash", "project", "child"], {
            cwd: ws.cwd,
        });

        expect(base).toHaveFailed();
        expect(base).toHaveStderrContaining("no project found");
        expect(child).toHaveSucceeded();
    });

    it("merges `extends` base config into the child", async () => {
        const ws = makeWorkspace(extendsSpec());

        // `from-base` is only defined on the base; the child gets it via extends.
        const result = await runOmni(
            ["-l", "off", "run", "from-base", "-p", "child"],
            {
                cwd: ws.cwd,
            },
        );

        expect(result).toHaveSucceeded();
        expect(result).toOutputContaining("from base");
    });

    it("resolves both short-form and long-form task definitions", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            projects: {
                app: {
                    name: "app",
                    tasks: {
                        short: 'echo "short form ran"',
                        long: { exec: 'echo "long form ran"' },
                    },
                },
            },
        });

        const short = await runOmni(["-l", "off", "run", "short"], {
            cwd: ws.cwd,
        });
        const long = await runOmni(["-l", "off", "run", "long"], {
            cwd: ws.cwd,
        });

        expect(short).toOutputContaining("short form ran");
        expect(long).toOutputContaining("long form ran");
    });
});

describe("+context @config (errors & env files)", () => {
    it("reports an actionable error for invalid config", async () => {
        const ws = makeWorkspace({ workspace: { projects: ["**"] } });
        // Schema-violating / malformed YAML in a project file.
        ws.write(
            "bad/project.omni.yaml",
            "name: bad\ntasks:\n  build: [oops: not\n",
        );

        const result = await runOmni(["-l", "off", "run", "build"], {
            cwd: ws.cwd,
        });

        expect(result).toHaveFailed();
        // The parse error should name the offending location, not just panic.
        expect(result.stderr).toMatch(/line \d+/);
    });

    it("loads the default env-file list with documented precedence", async () => {
        const ws = makeWorkspace({
            workspace: { projects: ["**"] },
            files: {
                ".env": "P=env\nA=1\n",
                ".env.local": "P=local\nB=2\n",
                ".env.development": "P=dev\nC=3\n",
                ".env.development.local": "P=devlocal\nD=4\n",
            },
        });

        const result = await runOmni(["-l", "off", "env", "all"], {
            cwd: ws.cwd,
        });
        const out = result.out;

        expect(result).toHaveSucceeded();
        // All four default files (.env, .env.local, .env.{ENV}, .env.{ENV}.local)
        // contribute their unique keys.
        expect(out).toContain("A=1");
        expect(out).toContain("B=2");
        expect(out).toContain("C=3");
        expect(out).toContain("D=4");
        // `.env.{ENV}.local` wins the precedence chain.
        expect(out).toContain("P=devlocal");
    });
});
