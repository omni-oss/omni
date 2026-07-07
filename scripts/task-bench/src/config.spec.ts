import { describe, expect, it } from "vitest";
import { resolveConfig } from "./config";
import { taskNames } from "./model";

describe("resolveConfig", () => {
    it("applies defaults", () => {
        const config = resolveConfig();
        expect(config.projects).toBe(50);
        expect(config.tasksPerProject).toBe(3);
        expect(config.dependency.strategy).toBe("layered");
        expect(config.tools).toEqual(["omni", "turbo", "nx", "moon"]);
        expect(config.projectNameTemplate).toBe("p-{project_id}");
    });

    it("merges partial overrides", () => {
        const config = resolveConfig({
            projects: 8,
            dependency: { strategy: "chain" },
        });
        expect(config.projects).toBe(8);
        expect(config.dependency.strategy).toBe("chain");
        // Untouched nested defaults remain.
        expect(config.dependency.fanout).toBe(3);
    });
});

describe("taskNames", () => {
    it("generates t0..tN-1", () => {
        expect(taskNames(resolveConfig({ tasksPerProject: 3 }))).toEqual([
            "t0",
            "t1",
            "t2",
        ]);
    });
});
