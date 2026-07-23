import { describe, expect, test, vi } from "vitest";
import { CapabilityPolicy } from "./capability-policy";
import { createEnforcedFetch, NetworkPolicyError } from "./enforced-net";

function fakeFetch() {
    return vi.fn(async () => new Response("ok"));
}

/** A single-level net residual in the layered `--enforce` wire format. */
function netPolicy(rules: { allow?: string[]; deny?: string[] }): string {
    return JSON.stringify({ enforced: ["net"], layers: [{ net: rules }] });
}

describe("createEnforcedFetch", () => {
    test("returns the base fetch unwrapped when net is not enforced", () => {
        const base = fakeFetch();
        const wrapped = createEnforcedFetch(
            base as unknown as typeof fetch,
            CapabilityPolicy.empty(),
        );
        expect(wrapped).toBe(base);
    });

    test("allows a permitted host and delegates to the base fetch", async () => {
        const base = fakeFetch();
        const policy = CapabilityPolicy.parse(
            netPolicy({ allow: ["example.com:443"] }),
        );
        const wrapped = createEnforcedFetch(
            base as unknown as typeof fetch,
            policy,
        );

        await wrapped("https://example.com/data");
        expect(base).toHaveBeenCalledOnce();
    });

    test("rejects a denied host before touching the base fetch", async () => {
        const base = fakeFetch();
        const policy = CapabilityPolicy.parse(
            netPolicy({ allow: ["example.com:443"] }),
        );
        const wrapped = createEnforcedFetch(
            base as unknown as typeof fetch,
            policy,
        );

        await expect(wrapped("https://evil.example/steal")).rejects.toThrow(
            NetworkPolicyError,
        );
        expect(base).not.toHaveBeenCalled();
    });

    test("derives the default port from the protocol", async () => {
        const base = fakeFetch();
        // Only :443 is allowed; an http:// (port 80) URL to the same host is denied.
        const policy = CapabilityPolicy.parse(
            netPolicy({ allow: ["example.com:443"] }),
        );
        const wrapped = createEnforcedFetch(
            base as unknown as typeof fetch,
            policy,
        );

        await wrapped("https://example.com/ok");
        await expect(wrapped("http://example.com/nope")).rejects.toThrow(
            NetworkPolicyError,
        );
        expect(base).toHaveBeenCalledOnce();
    });
});
