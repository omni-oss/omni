import { describe, expect, test } from "vitest";
import { RUNTIME, type Runtime } from "./runtime";

describe("RUNTIME", () => {
    test("is one of the supported runtime tags", () => {
        const allowed: Runtime[] = ["node", "bun", "deno"];
        expect(allowed).toContain(RUNTIME);
    });

    test("matches the host runtime detected via globals", () => {
        // The test process either has Deno, Bun, or neither as a global.
        const g = globalThis as Record<string, unknown>;
        const expected: Runtime =
            typeof g.Deno !== "undefined"
                ? "deno"
                : typeof g.Bun !== "undefined"
                  ? "bun"
                  : "node";
        expect(RUNTIME).toBe(expected);
    });
});
