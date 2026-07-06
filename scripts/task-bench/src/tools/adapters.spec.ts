import { describe, expect, it } from "vitest";
import { resolveConfig } from "../config";
import {
    assertSupportedVersion,
    describeTool,
    getAdapter,
    type ToolContext,
} from "./index";

const ctx = (daemon: boolean): ToolContext => ({
    rootDir: "/tmp/x",
    projectDirs: ["packages/a"],
    concurrency: 4,
    daemon,
});

describe("tool adapters (runtime)", () => {
    it("omni has no daemon and pins concurrency", () => {
        const omni = getAdapter("omni");
        expect(omni.hasDaemon).toBe(false);
        expect(omni.run("t2", ctx(true)).args).toEqual([
            "run",
            "t2",
            "-u",
            "stream",
            "-c",
            "4",
        ]);
        expect(omni.env(ctx(true))).toEqual({});
    });

    it("turbo toggles the daemon via run flags", () => {
        const turbo = getAdapter("turbo");
        expect(turbo.hasDaemon).toBe(true);
        expect(turbo.run("t2", ctx(true)).args).toContain("--daemon");
        expect(turbo.run("t2", ctx(true)).args).toContain("--concurrency=4");
        expect(turbo.run("t2", ctx(false)).args).toContain("--no-daemon");
    });

    it("nx toggles the daemon via NX_DAEMON and pins parallelism", () => {
        const nx = getAdapter("nx");
        expect(nx.hasDaemon).toBe(true);
        expect(nx.run("t2", ctx(true)).args).toContain("--parallel=4");
        expect(nx.env(ctx(true)).NX_DAEMON).toBe("true");
        expect(nx.env(ctx(false)).NX_DAEMON).toBe("false");
    });

    it("moon has no daemon and runs the :task target", () => {
        const moon = getAdapter("moon");
        expect(moon.hasDaemon).toBe(false);
        expect(moon.run("t2", ctx(true)).args).toEqual([
            "run",
            ":t2",
            "--concurrency",
            "4",
        ]);
    });
});

describe("tool adapters (versions & dependencies)", () => {
    const config = resolveConfig();

    it("declares supported version ranges and derives deps from config", () => {
        expect(getAdapter("turbo").devDependencies(config)).toEqual({
            turbo: config.versions.turbo,
        });
        expect(getAdapter("nx").devDependencies(config)).toEqual({
            nx: config.versions.nx,
        });
        expect(getAdapter("moon").devDependencies(config)).toEqual({
            "@moonrepo/cli": config.versions.moon,
        });
        // omni is external and installs nothing.
        expect(getAdapter("omni").devDependencies(config)).toEqual({});
        expect(getAdapter("omni").pinnedVersion(config)).toBeNull();
    });

    it("accepts the default pinned versions", () => {
        for (const tool of ["turbo", "nx", "moon"] as const) {
            const adapter = getAdapter(tool);
            const version = adapter.pinnedVersion(config);
            expect(version).not.toBeNull();
            expect(() =>
                assertSupportedVersion(adapter, version as string),
            ).not.toThrow();
        }
    });

    it("rejects an unsupported version", () => {
        expect(() =>
            assertSupportedVersion(getAdapter("turbo"), "1.5.0"),
        ).toThrow(/not supported/);
        expect(() =>
            assertSupportedVersion(getAdapter("nx"), "19.0.0"),
        ).toThrow(/not supported/);
        expect(() =>
            assertSupportedVersion(getAdapter("moon"), "1.0.0"),
        ).toThrow(/not supported/);
    });
});

describe("describeTool", () => {
    const config = resolveConfig();

    it("summarizes daemon + provisioning for a workspace-installed tool", () => {
        const info = describeTool("turbo", config, "2.10.3");
        expect(info).toMatchObject({
            tool: "turbo",
            version: "2.10.3",
            daemon: true,
            provisioning: "workspace-dependency",
            supportedVersions: ["^2.0.0"],
        });
        expect(info.description).toBeTruthy();
    });

    it("marks omni as a host binary with no daemon", () => {
        const info = describeTool("omni", config, null);
        expect(info.daemon).toBe(false);
        expect(info.provisioning).toBe("host-binary");
        expect(info.version).toBeNull();
    });

    it("reflects each runner's daemon attribute", () => {
        expect(describeTool("nx", config, "23.0.1").daemon).toBe(true);
        expect(describeTool("moon", config, "2.3.5").daemon).toBe(false);
    });
});
