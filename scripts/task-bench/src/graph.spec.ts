import { describe, expect, it } from "vitest";
import { resolveConfig } from "./config";
import { buildGraph, projectName, taskNames } from "./graph";

describe("resolveConfig", () => {
    it("applies defaults", () => {
        const config = resolveConfig();
        expect(config.projects).toBe(50);
        expect(config.tasksPerProject).toBe(3);
        expect(config.dependency.strategy).toBe("layered");
        expect(config.tools).toEqual(["omni", "turbo", "nx", "moon"]);
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

describe("projectName", () => {
    it("zero-pads to a stable width", () => {
        const config = resolveConfig({
            projects: 12,
            projectPrefix: "bench-p",
        });
        expect(projectName(config, 0)).toBe("bench-p0000");
        expect(projectName(config, 11)).toBe("bench-p0011");
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

describe("buildGraph strategies", () => {
    const base = { projects: 10 } as const;

    it("isolated has no edges", () => {
        const nodes = buildGraph(
            resolveConfig({ ...base, dependency: { strategy: "isolated" } }),
        );
        expect(nodes.every((n) => n.dependencies.length === 0)).toBe(true);
    });

    it("chain links each project to its predecessor", () => {
        const nodes = buildGraph(
            resolveConfig({ ...base, dependency: { strategy: "chain" } }),
        );
        expect(nodes[0]?.dependencies).toEqual([]);
        expect(nodes[5]?.dependencies).toEqual([4]);
    });

    it("fan-out points every project at the root", () => {
        const nodes = buildGraph(
            resolveConfig({ ...base, dependency: { strategy: "fan-out" } }),
        );
        expect(nodes[0]?.dependencies).toEqual([]);
        for (let i = 1; i < nodes.length; i++) {
            expect(nodes[i]?.dependencies).toEqual([0]);
        }
    });

    it("layered only depends on the previous layer", () => {
        const config = resolveConfig({
            projects: 10,
            dependency: { strategy: "layered", layers: 5, fanout: 2 },
        });
        const nodes = buildGraph(config);
        const perLayer = Math.ceil(10 / 5); // 2
        for (const node of nodes) {
            const layer = Math.floor(node.index / perLayer);
            for (const dep of node.dependencies) {
                expect(Math.floor(dep / perLayer)).toBe(layer - 1);
            }
        }
    });

    it("is always acyclic (deps point to lower indices)", () => {
        for (const strategy of [
            "chain",
            "fan-out",
            "layered",
            "random",
        ] as const) {
            const nodes = buildGraph(
                resolveConfig({ projects: 25, dependency: { strategy } }),
            );
            for (const node of nodes) {
                for (const dep of node.dependencies) {
                    expect(dep).toBeLessThan(node.index);
                }
            }
        }
    });

    it("random is deterministic for a fixed seed", () => {
        const mk = () =>
            buildGraph(
                resolveConfig({
                    projects: 30,
                    seed: 7,
                    dependency: { strategy: "random", edgeProbability: 0.4 },
                }),
            );
        expect(mk()).toEqual(mk());
    });
});
