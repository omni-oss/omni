import { describe, expect, test } from "vitest";
import {
    CapabilityFilteredEnv,
    EnvAccessDeniedError,
    type EnvRuleLayers,
    envLayersAllow,
    matchEnvGlob,
} from "./env-capability";

describe("matchEnvGlob", () => {
    test("matches an exact literal name", () => {
        expect(matchEnvGlob("PATH", "PATH")).toBe(true);
        expect(matchEnvGlob("PATH", "PATHEXT")).toBe(false);
    });

    test("`*` matches any run of characters (including none)", () => {
        expect(matchEnvGlob("MY_*", "MY_TOKEN")).toBe(true);
        expect(matchEnvGlob("MY_*", "MY_")).toBe(true);
        expect(matchEnvGlob("*_TOKEN", "AWS_TOKEN")).toBe(true);
        expect(matchEnvGlob("*", "ANYTHING")).toBe(true);
        expect(matchEnvGlob("MY_*", "OTHER")).toBe(false);
    });

    test("`?` matches exactly one character", () => {
        expect(matchEnvGlob("VAR?", "VAR1")).toBe(true);
        expect(matchEnvGlob("VAR?", "VAR")).toBe(false);
        expect(matchEnvGlob("VAR?", "VAR12")).toBe(false);
    });

    test("has no path-separator awareness (`*` crosses `/`)", () => {
        // Env names are opaque strings, not paths — `*` must span everything,
        // matching the Rust `glob_str_matches` (literal_separator: false).
        expect(matchEnvGlob("A*B", "A/x/B")).toBe(true);
    });

    test("treats regex metacharacters literally", () => {
        expect(matchEnvGlob("A.B", "A.B")).toBe(true);
        expect(matchEnvGlob("A.B", "AxB")).toBe(false);
    });
});

describe("envLayersAllow", () => {
    test("an empty stack denies everything (fail-closed)", () => {
        expect(envLayersAllow([], "PATH")).toBe(false);
    });

    test("a single allow grants a matching name and denies the rest", () => {
        const layers: EnvRuleLayers = [{ allow: ["PATH", "HOME"], deny: [] }];
        expect(envLayersAllow(layers, "PATH")).toBe(true);
        expect(envLayersAllow(layers, "HOME")).toBe(true);
        expect(envLayersAllow(layers, "SECRET")).toBe(false);
    });

    test("a glob allow matches by pattern", () => {
        const layers: EnvRuleLayers = [{ allow: ["MY_*"], deny: [] }];
        expect(envLayersAllow(layers, "MY_TOKEN")).toBe(true);
        expect(envLayersAllow(layers, "OTHER")).toBe(false);
    });

    test("a matching deny at any level dominates an allow", () => {
        const layers: EnvRuleLayers = [
            { allow: ["*"], deny: [] },
            { allow: [], deny: ["*_TOKEN"] },
        ];
        expect(envLayersAllow(layers, "PATH")).toBe(true);
        expect(envLayersAllow(layers, "AWS_TOKEN")).toBe(false);
    });

    test("an inner level cannot widen past an outer allow-list (ceiling)", () => {
        const layers: EnvRuleLayers = [
            { allow: ["APP_*"], deny: [] },
            { allow: ["APP_KEY", "SECRET"], deny: [] },
        ];
        // Granted by both the outer ceiling and the inner level.
        expect(envLayersAllow(layers, "APP_KEY")).toBe(true);
        // Granted by the inner level but outside the outer ceiling → blocked.
        expect(envLayersAllow(layers, "SECRET")).toBe(false);
        // Inside the outer ceiling but not granted by the (whitelisting) inner
        // level → blocked.
        expect(envLayersAllow(layers, "APP_OTHER")).toBe(false);
    });

    test("a silent inner level inherits the outer grant unchanged", () => {
        const layers: EnvRuleLayers = [
            { allow: ["PATH"], deny: [] },
            { allow: [], deny: [] },
        ];
        expect(envLayersAllow(layers, "PATH")).toBe(true);
        expect(envLayersAllow(layers, "OTHER")).toBe(false);
    });
});

describe("CapabilityFilteredEnv", () => {
    const raw = { PATH: "/bin", HOME: "/home/x", AWS_TOKEN: "secret" };

    test("get returns allowed values and null for denied names", () => {
        const env = new CapabilityFilteredEnv(
            raw,
            [{ allow: ["PATH", "HOME"], deny: [] }],
            "return-null",
        );
        expect(env.get("PATH")).toBe("/bin");
        expect(env.get("HOME")).toBe("/home/x");
        expect(env.get("AWS_TOKEN")).toBeNull();
    });

    test("get returns null for an allowed-but-unset name", () => {
        const env = new CapabilityFilteredEnv(raw, [
            { allow: ["MISSING"], deny: [] },
        ]);
        expect(env.get("MISSING")).toBeNull();
    });

    test("toObject exposes only the permitted variables", () => {
        const env = new CapabilityFilteredEnv(raw, [
            { allow: ["PATH", "HOME"], deny: [] },
        ]);
        expect(env.toObject()).toEqual({ PATH: "/bin", HOME: "/home/x" });
    });

    test("a deny hides a variable an allow would otherwise expose", () => {
        const env = new CapabilityFilteredEnv(
            raw,
            [{ allow: ["*"], deny: ["*_TOKEN"] }],
            "return-null",
        );
        expect(env.toObject()).toEqual({ PATH: "/bin", HOME: "/home/x" });
        expect(env.get("AWS_TOKEN")).toBeNull();
    });

    test("an empty policy hides everything (fail-closed)", () => {
        const env = new CapabilityFilteredEnv(raw, [], "return-null");
        expect(env.toObject()).toEqual({});
        expect(env.get("PATH")).toBeNull();
    });

    describe("onDenied", () => {
        test("defaults to throw", () => {
            const env = new CapabilityFilteredEnv(raw, [
                { allow: ["PATH"], deny: [] },
            ]);
            expect(() => env.get("AWS_TOKEN")).toThrow(EnvAccessDeniedError);
            // Permitted reads are unaffected by the default.
            expect(env.get("PATH")).toBe("/bin");
        });

        test("return-null returns null for a denied read", () => {
            const env = new CapabilityFilteredEnv(
                raw,
                [{ allow: ["PATH"], deny: [] }],
                "return-null",
            );
            expect(env.get("AWS_TOKEN")).toBeNull();
            expect(env.get("PATH")).toBe("/bin");
        });

        test("throw raises EnvAccessDeniedError for a denied read", () => {
            const env = new CapabilityFilteredEnv(
                raw,
                [{ allow: ["PATH"], deny: [] }],
                "throw",
            );
            expect(() => env.get("AWS_TOKEN")).toThrow(EnvAccessDeniedError);
            try {
                env.get("AWS_TOKEN");
            } catch (err) {
                expect(err).toBeInstanceOf(EnvAccessDeniedError);
                expect((err as EnvAccessDeniedError).variableName).toBe(
                    "AWS_TOKEN",
                );
            }
        });

        test("throw still returns permitted values normally", () => {
            const env = new CapabilityFilteredEnv(
                raw,
                [{ allow: ["PATH"], deny: [] }],
                "throw",
            );
            expect(env.get("PATH")).toBe("/bin");
            // An allowed-but-unset name is a normal null, not a denial.
            expect(
                new CapabilityFilteredEnv(
                    raw,
                    [{ allow: ["MISSING"], deny: [] }],
                    "throw",
                ).get("MISSING"),
            ).toBeNull();
        });

        test("toObject never throws regardless of onDenied", () => {
            const env = new CapabilityFilteredEnv(
                raw,
                [{ allow: ["PATH", "HOME"], deny: [] }],
                "throw",
            );
            expect(env.toObject()).toEqual({ PATH: "/bin", HOME: "/home/x" });
        });
    });
});
