import { afterEach, describe, expect, it } from "vitest";
import {
    ALIAS_ENV_KEY,
    canonicalScenario,
    resolveScenarioAliases,
} from "./scenario-aliases";

describe("resolveScenarioAliases", () => {
    afterEach(() => {
        delete process.env[ALIAS_ENV_KEY];
    });

    it("returns the constant baseline when the env is unset", () => {
        const map = resolveScenarioAliases({});
        expect(map).toEqual({});
    });

    it("merges the env override over the constant, env winning", () => {
        const map = resolveScenarioAliases({
            [ALIAS_ENV_KEY]: JSON.stringify({
                "scale-300": "scale-300projects",
            }),
        });
        expect(map["scale-300"]).toBe("scale-300projects");
    });

    it("throws on invalid JSON", () => {
        expect(() =>
            resolveScenarioAliases({ [ALIAS_ENV_KEY]: "{not json" }),
        ).toThrow(/not valid JSON/);
    });

    it("throws when the override is not an object of strings", () => {
        expect(() =>
            resolveScenarioAliases({ [ALIAS_ENV_KEY]: JSON.stringify(["a"]) }),
        ).toThrow(/must be a JSON object/);
        expect(() =>
            resolveScenarioAliases({
                [ALIAS_ENV_KEY]: JSON.stringify({ a: 1 }),
            }),
        ).toThrow(/must be a string/);
    });

    it("canonicalScenario maps aliases and passes through unmapped names", () => {
        const aliases = { old: "new" };
        expect(canonicalScenario("old", aliases)).toBe("new");
        expect(canonicalScenario("other", aliases)).toBe("other");
    });
});
