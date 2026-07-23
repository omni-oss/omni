import type { FetchFn } from "@omni-oss/gen-sdk-core";
import type { CapabilityPolicy } from "./capability-policy";

/** Default ports for the protocols a `fetch` request may use. */
const DEFAULT_PORTS: Readonly<Record<string, number>> = {
    "http:": 80,
    "https:": 443,
    "ws:": 80,
    "wss:": 443,
    "ftp:": 21,
};

/** Thrown when a request is refused by the `net` capability policy. */
export class NetworkPolicyError extends Error {
    constructor(host: string, port: number) {
        super(
            `capability policy denied network access to ${host}:${port} ` +
                `(not permitted by this generator's \`net\` policy)`,
        );
        this.name = "NetworkPolicyError";
    }
}

/**
 * Wrap a `fetch` so every request is authorized against the `net` policy before
 * a connection is attempted. When the policy does not enforce `net` (the runtime
 * confines it precisely at launch), `base` is returned unwrapped — zero overhead.
 */
export function createEnforcedFetch(
    base: FetchFn,
    policy: CapabilityPolicy,
): FetchFn {
    if (!policy.hasNet()) {
        return base;
    }

    const enforced: FetchFn = async (input, init) => {
        const url = requestUrl(input);
        const host = url.hostname;
        const port = requestPort(url);
        if (!policy.checkNet(host, port)) {
            throw new NetworkPolicyError(host, port);
        }
        return base(input, init);
    };

    return enforced;
}

/** Extract the target URL from any `fetch` first-argument form. */
function requestUrl(input: RequestInfo | URL): URL {
    if (typeof input === "string") {
        return new URL(input);
    }
    if (input instanceof URL) {
        return input;
    }
    // `Request`-like: has a `url` string.
    return new URL(input.url);
}

/**
 * The effective port of a request: the explicit port, else the protocol's
 * default. Falls back to `0` for unknown schemes (a numeric port pattern then
 * cannot match, which is the fail-closed choice).
 */
function requestPort(url: URL): number {
    if (url.port !== "") {
        return Number.parseInt(url.port, 10);
    }
    return DEFAULT_PORTS[url.protocol] ?? 0;
}
