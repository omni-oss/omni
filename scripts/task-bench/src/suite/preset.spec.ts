import { describe, expect, it } from "vitest";
import {
    deepMerge,
    getPreset,
    listPresets,
    parseSuite,
    resolveScenario,
} from "./preset";

describe("deepMerge", () => {
    it("recursively merges plain objects and replaces scalars/arrays", () => {
        expect(
            deepMerge(
                { a: 1, nested: { x: 1, y: 2 }, list: [1, 2] },
                { a: 2, nested: { y: 3, z: 4 }, list: [9] },
            ),
        ).toEqual({ a: 2, nested: { x: 1, y: 3, z: 4 }, list: [9] });
    });
});

describe("built-in presets", () => {
    it("exposes the expected presets", () => {
        expect(listPresets()).toEqual(
            expect.arrayContaining([
                "quick",
                "shapes",
                "scale",
                "density",
                "daemon",
                "full",
            ]),
        );
    });

    it("throws a helpful error for an unknown preset", () => {
        expect(() => getPreset("nope")).toThrow(/unknown preset/);
    });

    it("every built-in preset parses and resolves", () => {
        for (const name of listPresets()) {
            const suite = getPreset(name);
            expect(suite.displayName).toBeTruthy();
            for (const scenario of suite.scenarios) {
                const resolved = resolveScenario(suite, scenario);
                expect(resolved.config.projects).toBeGreaterThan(0);
                expect(resolved.displayName).toBeTruthy();
            }
        }
    });
});

describe("resolveScenario", () => {
    it("merges suite defaults under scenario overrides", () => {
        const suite = getPreset("shapes");
        const chain = suite.scenarios.find((s) => s.name === "shape-chain");
        expect(chain).toBeDefined();
        // biome-ignore lint/style/noNonNullAssertion: for testing
        const resolved = resolveScenario(suite, chain!);
        // From defaults:
        expect(resolved.config.projects).toBe(120);
        expect(resolved.run.concurrency).toBe(8);
        // From the scenario override:
        expect(resolved.config.dependency.strategy).toBe("chain");
    });

    it("carries per-scenario run overrides (daemon on/off)", () => {
        const suite = getPreset("daemon");
        const off = suite.scenarios.find((s) => s.name === "daemon-off");
        // biome-ignore lint/style/noNonNullAssertion: for testing
        expect(resolveScenario(suite, off!).run.daemon).toBe(false);
    });

    it("resolves displayName, falling back to name when omitted", () => {
        const suite = parseSuite({
            name: "custom",
            scenarios: [
                { name: "a", displayName: "Scenario A", config: {} },
                { name: "b", config: {} },
            ],
        });
        // biome-ignore lint/style/noNonNullAssertion: for testing
        expect(resolveScenario(suite, suite.scenarios[0]!).displayName).toBe(
            "Scenario A",
        );
        // biome-ignore lint/style/noNonNullAssertion: for testing
        expect(resolveScenario(suite, suite.scenarios[1]!).displayName).toBe(
            "b",
        );
    });
});

describe("parseSuite", () => {
    it("validates a custom suite object", () => {
        const suite = parseSuite({
            name: "custom",
            scenarios: [
                { name: "a", config: { projects: 5 } },
                { name: "b", config: { projects: 6, tasksPerProject: 2 } },
            ],
        });
        expect(suite.scenarios).toHaveLength(2);
        expect(
            // biome-ignore lint/style/noNonNullAssertion: for testing
            resolveScenario(suite, suite.scenarios[0]!).config.projects,
        ).toBe(5);
    });

    it("rejects filesystem-unsafe scenario names", () => {
        expect(() =>
            parseSuite({
                scenarios: [{ name: "bad/name", config: {} }],
            }),
        ).toThrow();
    });
});
