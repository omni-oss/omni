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

describe("createEnforcedFetch — redirect re-authorization", () => {
    /** A base fetch that 302-redirects to `location` on its first call, then 200s. */
    function redirectingFetch(location: string) {
        let call = 0;
        return vi.fn(async () => {
            if (call++ === 0) {
                return new Response(null, {
                    status: 302,
                    headers: { location },
                });
            }
            return new Response("ok");
        });
    }

    test("follows a redirect to an allowed host and re-authorizes it", async () => {
        const base = redirectingFetch("https://cdn.example/asset");
        const policy = CapabilityPolicy.parse(
            netPolicy({ allow: ["origin.example:443", "cdn.example:443"] }),
        );
        const wrapped = createEnforcedFetch(
            base as unknown as typeof fetch,
            policy,
        );
        const res = await wrapped("https://origin.example/start");
        expect(await res.text()).toBe("ok");
        // Both hops were fetched: the origin and the (allowed) redirect target.
        expect(base).toHaveBeenCalledTimes(2);
    });

    test("blocks a redirect to a denied host before connecting to it", async () => {
        const base = redirectingFetch("https://evil.example/steal");
        // Only the origin is allowed; the redirect target is not.
        const policy = CapabilityPolicy.parse(
            netPolicy({ allow: ["origin.example:443"] }),
        );
        const wrapped = createEnforcedFetch(
            base as unknown as typeof fetch,
            policy,
        );
        await expect(wrapped("https://origin.example/start")).rejects.toThrow(
            NetworkPolicyError,
        );
        // The origin was fetched once; the connection to the denied host is
        // refused *before* the second fetch happens.
        expect(base).toHaveBeenCalledTimes(1);
    });

    test("passes an explicit `redirect: manual` through without following", async () => {
        const base = redirectingFetch("https://evil.example/steal");
        const policy = CapabilityPolicy.parse(
            netPolicy({ allow: ["origin.example:443"] }),
        );
        const wrapped = createEnforcedFetch(
            base as unknown as typeof fetch,
            policy,
        );
        // The caller manages redirects: the 302 is handed back, not followed, so
        // the denied host is never contacted by us.
        const res = await wrapped("https://origin.example/start", {
            redirect: "manual",
        });
        expect(res.status).toBe(302);
        expect(base).toHaveBeenCalledTimes(1);
    });
});
