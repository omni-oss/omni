import { describe, expect, it } from "vitest";

import { createConfig } from "./base.ts";

describe("dependency externalization", () => {
    it.each(["dependencies", "devDependencies", "peerDependencies"] as const)(
        "externalizes %s and their subpaths without matching similarly named packages",
        (dependencyType) => {
            const config = createConfig({
                package: {
                    [dependencyType]: { example: "*", "@scope/example": "*" },
                },
                externalizeDeps: { [dependencyType]: true },
            });
            const external = config.build?.rolldownOptions?.external;
            if (typeof external !== "function") {
                throw new Error("Expected an externalization predicate");
            }

            for (const id of [
                "example",
                "example/nested/module",
                "@scope/example",
                "@scope/example/subpath",
            ]) {
                expect(external(id, undefined, false)).toBe(true);
            }
            for (const id of [
                "example-other",
                "@scope/example-other/subpath",
            ]) {
                expect(external(id, undefined, false)).toBe(false);
            }
        },
    );

    it("keeps explicitly listed dependency IDs exact", () => {
        const config = createConfig({
            package: { dependencies: { example: "*" } },
            externalizeDeps: { dependencies: ["example"] },
        });
        const external = config.build?.rolldownOptions?.external;
        if (typeof external !== "function") {
            throw new Error("Expected an externalization predicate");
        }

        expect(external("example", undefined, false)).toBe(true);
        expect(external("example/subpath", undefined, false)).toBe(false);
    });

    it("does not externalize disabled dependency groups", () => {
        const config = createConfig({
            package: { dependencies: { example: "*" } },
            externalizeDeps: { dependencies: false },
        });
        const external = config.build?.rolldownOptions?.external;
        if (typeof external !== "function") {
            throw new Error("Expected an externalization predicate");
        }

        expect(external("example", undefined, false)).toBe(false);
        expect(external("example/subpath", undefined, false)).toBe(false);
    });
});
