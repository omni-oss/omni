/**
 * Smoke tests for the e2e harness itself.
 *
 * These don't pin omni behavior (that's what the per-area suites are for) -
 * they just prove the harness wiring works: the binary resolves, runOmni
 * captures output/exit codes, makeWorkspace builds an isolated tree, and the
 * custom matchers are registered.
 */

import { describe, expect, it } from "vitest";
import {
    dependencyChainSpec,
    makeWorkspace,
    multiFormatProjectsSpec,
    normalize,
    runOmni,
    spawnOmniPty,
} from "@/harness";

describe("harness", () => {
    it("runs omni --version and reports a clean exit", async () => {
        const result = await runOmni(["--version"]);

        expect(result).toHaveSucceeded();
        expect(result).toMatchOutput(/\d+\.\d+\.\d+/);
    });

    it("captures non-zero exit codes without throwing", async () => {
        const result = await runOmni(["definitely-not-a-subcommand"]);

        expect(result).toHaveFailed();
        expect(result.exitCode).not.toBe(0);
    });

    it("creates an isolated workspace with YAML configs by default", () => {
        const ws = makeWorkspace({
            projects: {
                app: { name: "app", tasks: { build: 'echo "hi"' } },
            },
        });

        expect(ws.exists("workspace.omni.yaml")).toBe(true);
        expect(ws.exists("app/project.omni.yaml")).toBe(true);
        expect(ws.read("app/project.omni.yaml")).toContain("name: app");
    });

    it("serializes object configs by file extension", () => {
        const ws = makeWorkspace({
            projects: {
                "json-app/project.omni.json": { name: "json-app" },
                "toml-app/project.omni.toml": { name: "toml-app" },
            },
        });

        expect(ws.read("json-app/project.omni.json")).toContain(
            '"name": "json-app"',
        );
        expect(ws.read("toml-app/project.omni.toml")).toContain(
            'name = "toml-app"',
        );
    });

    it("materializes reusable fixtures", () => {
        const ws = makeWorkspace(dependencyChainSpec());

        expect(ws.exists("project-1/project.omni.yaml")).toBe(true);
        expect(ws.exists("project-2/project.omni.yaml")).toBe(true);
    });

    it("materializes a project per config format", () => {
        const ws = makeWorkspace(multiFormatProjectsSpec());

        expect(ws.read("yaml-app/project.omni.yaml")).toContain(
            "name: yaml-app",
        );
        expect(ws.read("yml-app/project.omni.yml")).toContain("name: yml-app");
        expect(ws.read("json-app/project.omni.json")).toContain(
            '"name": "json-app"',
        );
        expect(ws.read("toml-app/project.omni.toml")).toContain(
            'name = "toml-app"',
        );
    });
});

describe("pty harness", () => {
    it("renders output to a screen and reports a clean exit", async () => {
        const pty = spawnOmniPty(["--version"]);

        await pty.waitFor(/\d+\.\d+\.\d+/);
        const result = await pty.waitForExit();

        expect(result.exitCode).toBe(0);
        expect(pty.exited).toBe(true);
        expect(pty.screen()).toMatch(/\d+\.\d+\.\d+/);
    });

    it("captures task output run under a tty", async () => {
        const ws = makeWorkspace({
            projects: {
                app: { name: "app", tasks: { build: "echo pty-build-ran" } },
            },
        });

        const pty = spawnOmniPty(["run", "build"], { cwd: ws.cwd });
        await pty.waitFor("pty-build-ran");

        expect(await pty.waitForExit()).toMatchObject({ exitCode: 0 });
    });

    it("rejects pending waits when disposed", async () => {
        const pty = spawnOmniPty(["--version"]);
        const pending = pty.waitFor("this output never appears");

        pty.dispose();

        await expect(pending).rejects.toThrow(/disposed/);
    });
});

describe("normalize", () => {
    it("collapses line endings and trims trailing newlines", () => {
        expect(normalize("a\r\nb\r\n\n")).toBe("a\nb");
    });
});
