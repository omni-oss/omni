import { describe, expect, it } from "vitest";
import { parse as parseYaml } from "yaml";
import { resolveConfig } from "../config";
import { buildGraph, type ProjectNode } from "../graph";
import { moonProjectConfig } from "./moon";
import { nxProjectConfig, nxRootConfig } from "./nx";
import { omniProjectConfig } from "./omni";
import { turboRootConfig } from "./turbo";
import { taskDependencies } from "./types";

describe("taskDependencies", () => {
    it("chains within a project and fans upstream by default", () => {
        const config = resolveConfig();
        expect(taskDependencies(config, 0)).toEqual(["^t0"]);
        expect(taskDependencies(config, 2)).toEqual(["t1", "^t2"]);
    });

    it("respects disabled chaining / fan-out", () => {
        const config = resolveConfig({
            task: { chainWithinProject: false, fanUpstream: false },
        });
        expect(taskDependencies(config, 2)).toEqual([]);
    });
});

describe("equivalence across runners", () => {
    const config = resolveConfig({
        projects: 6,
        tasksPerProject: 2,
        dependency: { strategy: "chain" },
    });
    const projects = buildGraph(config);
    const project = projects[3] as ProjectNode;
    const parent = projects[2] as ProjectNode;

    it("omni encodes the same task deps and project deps", () => {
        const doc = parseYaml(omniProjectConfig(config, project, projects));
        expect(doc.name).toBe(project.name);
        expect(doc.dependencies).toEqual([parent.name]);
        expect(doc.tasks.t1.dependencies).toEqual(["t0", "^t1"]);
        expect(doc.tasks.t1.cache.output.files).toEqual(["dist/t1.*"]);
    });

    it("turbo encodes matching dependsOn / outputs", () => {
        const turbo = JSON.parse(turboRootConfig(config));
        expect(turbo.tasks.t1.dependsOn).toEqual(["t0", "^t1"]);
        expect(turbo.tasks.t1.outputs).toEqual(["dist/t1.*"]);
        expect(turbo.globalPassThroughEnv).toContain("TASK_BENCH_EXEC_LOG");
    });

    it("nx encodes matching dependsOn / outputs and per-project targets", () => {
        const nx = JSON.parse(nxRootConfig(config));
        expect(nx.targetDefaults.t1.dependsOn).toEqual(["t0", "^t1"]);
        expect(nx.targetDefaults.t1.outputs).toEqual([
            "{projectRoot}/dist/t1.*",
        ]);
        expect(nx.targetDefaults.t1.cache).toBe(true);

        const proj = JSON.parse(nxProjectConfig(config, project));
        expect(proj.name).toBe(project.name);
        expect(proj.targets.t1.executor).toBe("nx:run-commands");
        expect(proj.targets.t1.options.command).toBe("node ./task.mjs t1");
    });

    it("moon encodes matching deps / outputs and project deps", () => {
        const doc = parseYaml(moonProjectConfig(config, project, projects));
        expect(doc.id).toBe(project.name);
        expect(doc.dependsOn).toEqual([parent.name]);
        expect(doc.tasks.t1.deps).toEqual(["~:t0", "^:t1"]);
        expect(doc.tasks.t1.command).toBe("node ./task.mjs t1");
        expect(doc.tasks.t1.outputs).toEqual(["dist/t1.*"]);
    });
});
