import { describe, expect, it } from "vitest";
import { resolveConfig } from "./config";
import { buildModel, expectedColdExecuted, modelVersion } from "./model";

describe("buildModel", () => {
    it("names projects and dirs via the template with digit padding", () => {
        const model = buildModel(
            resolveConfig({ projects: 12, tasksPerProject: 2 }),
        );
        expect(model.modelVersion).toBe(modelVersion());
        expect(model.projects[7]?.name).toBe("p-07");
        expect(model.projects[7]?.dir).toBe("packages/p-07");
    });

    it("resolves task edges (intra chain + upstream fan)", () => {
        const model = buildModel(
            resolveConfig({ projects: 2, tasksPerProject: 3 }),
        );
        const tasks = model.projects[0]?.tasks ?? [];
        expect(tasks[0]?.dependencies).toEqual(["^t0"]);
        expect(tasks[1]?.dependencies).toEqual(["t0", "^t1"]);
        expect(tasks[1]?.outputGlobs).toEqual(["dist/t1.*"]);
    });
});

describe("expectedColdExecuted", () => {
    it("counts each project's t0..tK prefix when chaining is on", () => {
        const model = buildModel(
            resolveConfig({ projects: 10, tasksPerProject: 3 }),
        );
        expect(expectedColdExecuted(model, "t2")).toBe(30);
        expect(expectedColdExecuted(model, "t0")).toBe(10);
    });

    it("counts one task per project when chaining is off", () => {
        const model = buildModel(
            resolveConfig({
                projects: 8,
                tasksPerProject: 4,
                task: { chainWithinProject: false },
            }),
        );
        expect(expectedColdExecuted(model, "t3")).toBe(8);
    });

    it("returns null for an unknown or out-of-range task", () => {
        const model = buildModel(
            resolveConfig({ projects: 5, tasksPerProject: 3 }),
        );
        expect(expectedColdExecuted(model, "build")).toBeNull();
        expect(expectedColdExecuted(model, "t3")).toBeNull();
    });
});
