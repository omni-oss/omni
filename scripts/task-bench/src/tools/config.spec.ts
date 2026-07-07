import { describe, expect, it } from "vitest";
import { parse as parseYaml } from "yaml";
import { resolveConfig } from "../config";
import { buildModel, type ProjectModel, renderOmni } from "../model";
import { moonProjectConfig } from "./moon";
import { nxProjectConfig, nxRootConfig } from "./nx";
import { OMNI_RENDER_OPTIONS } from "./omni";
import { turboRootConfig } from "./turbo";

describe("equivalence across runners", () => {
    const config = resolveConfig({
        projects: 6,
        tasksPerProject: 2,
        dependency: { strategy: "chain" },
    });
    const model = buildModel(config);
    const project = model.projects[3] as ProjectModel;
    const parent = model.projects[2] as ProjectModel;

    it("omni (shared core) encodes the same task deps and project deps", () => {
        const files = renderOmni(model, OMNI_RENDER_OPTIONS);
        const entry = files.find(
            ([rel]) => rel === `${project.dir}/project.omni.yaml`,
        );
        expect(entry).toBeDefined();
        const doc = parseYaml((entry as [string, string])[1]);
        expect(doc.name).toBe(project.name);
        expect(doc.dependencies).toEqual([parent.name]);
        expect(doc.tasks.t1.dependencies).toEqual(["t0", "^t1"]);
        expect(doc.tasks.t1.cache.output.files).toEqual(["dist/t1.*"]);
    });

    it("turbo encodes matching dependsOn / outputs", () => {
        const turbo = JSON.parse(turboRootConfig(model));
        expect(turbo.tasks.t1.dependsOn).toEqual(["t0", "^t1"]);
        expect(turbo.tasks.t1.outputs).toEqual(["dist/t1.*"]);
        expect(turbo.globalPassThroughEnv).toContain("TASK_BENCH_EXEC_LOG");
    });

    it("nx encodes matching dependsOn / outputs and per-project targets", () => {
        const nx = JSON.parse(nxRootConfig(model));
        expect(nx.targetDefaults.t1.dependsOn).toEqual(["t0", "^t1"]);
        expect(nx.targetDefaults.t1.outputs).toEqual([
            "{projectRoot}/dist/t1.*",
        ]);
        expect(nx.targetDefaults.t1.cache).toBe(true);

        const proj = JSON.parse(nxProjectConfig(project));
        expect(proj.name).toBe(project.name);
        expect(proj.targets.t1.executor).toBe("nx:run-commands");
        expect(proj.targets.t1.options.command).toBe("node ./task.mjs t1");
    });

    it("moon encodes matching deps / outputs and project deps", () => {
        const doc = parseYaml(moonProjectConfig(project));
        expect(doc.id).toBe(project.name);
        expect(doc.dependsOn).toEqual([parent.name]);
        expect(doc.tasks.t1.deps).toEqual(["~:t0", "^:t1"]);
        expect(doc.tasks.t1.command).toBe("node ./task.mjs t1");
        expect(doc.tasks.t1.outputs).toEqual(["dist/t1.*"]);
    });
});
