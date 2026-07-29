import { describe, expect, test, vi } from "vitest";
import { CapabilityPolicy } from "./capability-policy";
import { NetworkPolicyError } from "./enforced-net";
import { createEnforcedPreconnect } from "./index";

function netPolicy(...allow: string[]): CapabilityPolicy {
    return CapabilityPolicy.parse(
        JSON.stringify({ enforced: ["net"], layers: [{ net: { allow } }] }),
    );
}

describe("createEnforcedPreconnect", () => {
    test("authorizes the eager connection against the net policy", () => {
        const raw = vi.fn(() => "preconnected");
        const preconnect = createEnforcedPreconnect(
            raw,
            netPolicy("cdn.example:443"),
        );

        // A denied host is refused before the raw preconnect opens a socket.
        expect(() => preconnect("https://evil.example")).toThrow(
            NetworkPolicyError,
        );
        expect(raw).not.toHaveBeenCalled();

        // An allowed host reaches the raw implementation with its args intact.
        expect(preconnect("https://cdn.example")).toBe("preconnected");
        expect(raw).toHaveBeenCalledWith("https://cdn.example");
    });

    test("passes an un-parseable argument through to the raw implementation", () => {
        const raw = vi.fn(() => "ok");
        const preconnect = createEnforcedPreconnect(
            raw,
            netPolicy("cdn.example:443"),
        );
        // Not a URL → we do not guess a target; the raw impl decides.
        expect(preconnect("not a url")).toBe("ok");
        expect(raw).toHaveBeenCalledOnce();
    });
});
