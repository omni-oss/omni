import { afterEach, describe, expect, it } from "vitest";
import {
    CONSTANT_PRESET_ALIASES,
    canonicalPreset,
    PRESET_ALIAS_ENV_KEY,
    resolvePresetAliases,
} from "./preset-aliases";

describe("resolvePresetAliases", () => {
    afterEach(() => {
        delete process.env[PRESET_ALIAS_ENV_KEY];
    });

    it("returns the built-in baseline when the env is unset", () => {
        const map = resolvePresetAliases({});
        expect(map).toEqual(CONSTANT_PRESET_ALIASES);
        expect(map["full suite"]).toBe("full");
    });

    it("merges the env override over the baseline, env winning", () => {
        const map = resolvePresetAliases({
            [PRESET_ALIAS_ENV_KEY]: JSON.stringify({ "full run": "full" }),
        });
        expect(map["full run"]).toBe("full");
        // Baseline still present.
        expect(map["scale sweep"]).toBe("scale");
    });

    it("throws on invalid JSON", () => {
        expect(() =>
            resolvePresetAliases({ [PRESET_ALIAS_ENV_KEY]: "{nope" }),
        ).toThrow(/not valid JSON/);
    });

    it("throws when the override is not an object of strings", () => {
        expect(() =>
            resolvePresetAliases({
                [PRESET_ALIAS_ENV_KEY]: JSON.stringify({ a: 2 }),
            }),
        ).toThrow(/must be a string/);
    });

    it("canonicalPreset maps built-in names and passes through unmapped ones", () => {
        expect(canonicalPreset("full suite", CONSTANT_PRESET_ALIASES)).toBe(
            "full",
        );
        expect(canonicalPreset("full", CONSTANT_PRESET_ALIASES)).toBe("full");
        expect(canonicalPreset("custom", CONSTANT_PRESET_ALIASES)).toBe(
            "custom",
        );
    });
});
