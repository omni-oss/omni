import { describe, expect, test } from "vitest";
import { CapabilityPolicy, globMatches, netMatches } from "./capability-policy";

type Rules = { allow?: string[]; deny?: string[] };
type Layer = Record<string, Rules>;

/**
 * Build the layered `--enforce` JSON the Rust `ShimPolicy` emits: `enforced` is
 * derived from the union of domains named across the layers, and each argument
 * is one policy level (outermost first).
 */
function policyJson(...layers: Layer[]): string {
    const enforced = new Set<string>();
    for (const layer of layers) {
        for (const domain of Object.keys(layer)) {
            enforced.add(domain);
        }
    }
    return JSON.stringify({ enforced: [...enforced], layers });
}

/** A single-level policy (the common case). */
function single(layer: Layer): CapabilityPolicy {
    return CapabilityPolicy.parse(policyJson(layer));
}

describe("CapabilityPolicy.parse", () => {
    test("blank / invalid input yields an empty passthrough policy", () => {
        for (const input of [undefined, null, "", "   ", "not json", "[1,2]"]) {
            const p = CapabilityPolicy.parse(input as string);
            expect(p.hasNet()).toBe(false);
            expect(p.hasProcess()).toBe(false);
            // Passthrough: absent domain always allows (runtime enforces).
            expect(p.checkNet("anywhere.example", 443)).toBe(true);
            expect(p.checkProcess("anything")).toBe(true);
        }
    });

    test("recognises the net and process domains", () => {
        const p = single({
            net: { allow: ["example.com:443"] },
            process: { allow: ["git"] },
        });
        expect(p.hasNet()).toBe(true);
        expect(p.hasProcess()).toBe(true);
    });

    test("recognises the env domain", () => {
        const p = single({ env: { allow: ["PATH"] } });
        expect(p.hasEnv()).toBe(true);
        expect(p.hasNet()).toBe(false);
    });

    test("a domain enforced with no granting layer is deny-all", () => {
        // `process` is marked enforced but no layer grants it: the shim patches
        // the API and denies every call (the fold's fail-closed rule).
        const p = CapabilityPolicy.parse(
            JSON.stringify({ enforced: ["process"], layers: [] }),
        );
        expect(p.hasProcess()).toBe(true);
        expect(p.checkProcess("git")).toBe(false);
    });
});

describe("env policy", () => {
    test("absent env domain is passthrough (runtime/broker enforces)", () => {
        const p = single({ net: { allow: ["example.com:443"] } });
        expect(p.hasEnv()).toBe(false);
        expect(p.checkEnv("ANYTHING")).toBe(true);
        expect(p.envRuleLayers()).toBeUndefined();
    });

    test("enforced env gates by name with glob support", () => {
        const p = single({ env: { allow: ["PATH", "MY_*"] } });
        expect(p.hasEnv()).toBe(true);
        expect(p.checkEnv("PATH")).toBe(true);
        expect(p.checkEnv("MY_TOKEN")).toBe(true);
        expect(p.checkEnv("SECRET")).toBe(false);
    });

    test("a matching env deny is dominant", () => {
        const p = single({ env: { allow: ["*"], deny: ["*_TOKEN"] } });
        expect(p.checkEnv("PATH")).toBe(true);
        expect(p.checkEnv("AWS_TOKEN")).toBe(false);
    });

    test("env enforced with no granting layer is deny-all", () => {
        const p = CapabilityPolicy.parse(
            JSON.stringify({ enforced: ["env"], layers: [] }),
        );
        expect(p.hasEnv()).toBe(true);
        expect(p.checkEnv("PATH")).toBe(false);
    });

    test("env attenuates across levels (inner cannot widen)", () => {
        const p = CapabilityPolicy.parse(
            policyJson(
                { env: { allow: ["APP_*"] } },
                { env: { allow: ["APP_KEY", "SECRET"] } },
            ),
        );
        expect(p.checkEnv("APP_KEY")).toBe(true);
        // Granted by the inner level but outside the outer ceiling.
        expect(p.checkEnv("SECRET")).toBe(false);
    });

    test("envRuleLayers projects the neutral {allow,deny} shape per level", () => {
        const p = CapabilityPolicy.parse(
            policyJson(
                { env: { allow: ["APP_*"] } },
                { env: { allow: ["APP_KEY"], deny: ["APP_SECRET"] } },
            ),
        );
        expect(p.envRuleLayers()).toEqual([
            { allow: ["APP_*"], deny: [] },
            { allow: ["APP_KEY"], deny: ["APP_SECRET"] },
        ]);
    });
});

describe("net policy (deny-dominant, fail-closed)", () => {
    const p = single({
        net: {
            allow: ["example.com:443", "*.cdn.example:443", "api.local:*"],
            deny: ["secret.cdn.example:*"],
        },
    });

    test("exact host:port is allowed, others denied (fail-closed)", () => {
        expect(p.checkNet("example.com", 443)).toBe(true);
        expect(p.checkNet("evil.example", 443)).toBe(false);
    });

    test("port must match when the pattern pins one", () => {
        expect(p.checkNet("example.com", 80)).toBe(false);
    });

    test("`*` port allows any port", () => {
        expect(p.checkNet("api.local", 80)).toBe(true);
        expect(p.checkNet("api.local", 9999)).toBe(true);
    });

    test("host wildcard matches sub-labels but not the bare suffix", () => {
        expect(p.checkNet("a.cdn.example", 443)).toBe(true);
        expect(p.checkNet("cdn.example", 443)).toBe(false);
    });

    test("deny dominates a matching allow", () => {
        // `secret.cdn.example` matches the `*.cdn.example` allow, but the deny
        // wins regardless.
        expect(p.checkNet("secret.cdn.example", 443)).toBe(false);
    });
});

describe("process policy", () => {
    const p = single({
        process: { allow: ["git", "npm-*"], deny: ["npm-publish"] },
    });

    test("exact and glob allows", () => {
        expect(p.checkProcess("git")).toBe(true);
        expect(p.checkProcess("npm-install")).toBe(true);
        expect(p.checkProcess("rm")).toBe(false);
    });

    test("deny dominates", () => {
        expect(p.checkProcess("npm-publish")).toBe(false);
    });
});

describe("layered attenuation (shrink-only)", () => {
    test("an inner level cannot widen past an outer level's allow-list", () => {
        // Outer (workspace/parent) ceiling allows only *.example.com; inner
        // (child) tries to also allow evil.com. The inner widening is capped:
        // evil.com is blocked by the outer ceiling, example.com still allowed.
        const p = CapabilityPolicy.parse(
            policyJson(
                { net: { allow: ["*.example.com:443"] } },
                { net: { allow: ["evil.com:443", "api.example.com:443"] } },
            ),
        );
        expect(p.checkNet("api.example.com", 443)).toBe(true);
        expect(p.checkNet("evil.com", 443)).toBe(false);
    });

    test("an inner level narrows within the outer allow-list", () => {
        // Outer allows the whole subtree; inner narrows to a single host. Only
        // the intersection (api.example.com) is permitted.
        const p = CapabilityPolicy.parse(
            policyJson(
                { net: { allow: ["*.example.com:443"] } },
                { net: { allow: ["api.example.com:443"] } },
            ),
        );
        expect(p.checkNet("api.example.com", 443)).toBe(true);
        // Allowed by the outer ceiling but not granted by the inner level.
        expect(p.checkNet("cdn.example.com", 443)).toBe(false);
    });

    test("a silent inner level inherits the outer grant unchanged", () => {
        // A single non-empty outer layer plus an implicitly-silent inner level
        // (absent from `layers`): the outer grant flows through.
        const p = CapabilityPolicy.parse(
            policyJson({ net: { allow: ["example.com:443"] } }),
        );
        expect(p.checkNet("example.com", 443)).toBe(true);
        expect(p.checkNet("other.com", 443)).toBe(false);
    });

    test("a deny at any level dominates every other level's allow", () => {
        const p = CapabilityPolicy.parse(
            policyJson(
                { net: { deny: ["blocked.example:*"] } },
                { net: { allow: ["blocked.example:443", "ok.example:443"] } },
            ),
        );
        expect(p.checkNet("blocked.example", 443)).toBe(false);
        expect(p.checkNet("ok.example", 443)).toBe(true);
    });

    test("process attenuates across levels too", () => {
        const p = CapabilityPolicy.parse(
            policyJson(
                { process: { allow: ["git", "npm"] } },
                { process: { allow: ["git", "rm"] } },
            ),
        );
        expect(p.checkProcess("git")).toBe(true);
        // `rm` is granted by the inner level but not by the outer ceiling.
        expect(p.checkProcess("rm")).toBe(false);
        // `npm` is granted by the outer level but not by the inner one.
        expect(p.checkProcess("npm")).toBe(false);
    });
});

describe("matchers", () => {
    test("globMatches supports * and ? literally otherwise", () => {
        expect(globMatches("git", "git")).toBe(true);
        expect(globMatches("git", "gitx")).toBe(false);
        expect(globMatches("gi?", "git")).toBe(true);
        expect(globMatches("g*t", "great")).toBe(true);
        // Dots are literal, not regex wildcards.
        expect(globMatches("a.b", "axb")).toBe(false);
    });

    test("netMatches treats a bare host as any-port", () => {
        expect(netMatches("example.com", "example.com", 12345)).toBe(true);
        expect(netMatches("example.com", "other.com", 12345)).toBe(false);
    });
});
