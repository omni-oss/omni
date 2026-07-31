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
 *
 * Two subtleties beyond the initial check:
 *
 * * **TOCTOU** — the request target is snapshotted into an immutable, canonical
 *   {@link Request} *once* (via `new Request`, exactly how the runtime itself
 *   normalizes it) and that frozen object is both authorized and sent, so a
 *   caller cannot hand in a `URL`/`Request` whose target mutates between the
 *   check and the send.
 * * **Redirect SSRF** — on runtimes whose native `fetch` does not funnel through
 *   the JS socket patch (Deno/Bun), an allowed origin that 3xx-redirects to a
 *   denied host would connect there unseen. So redirects are followed *manually*
 *   and every `Location` hop is re-authorized. A caller that opted out of
 *   following (`redirect: "manual"`/`"error"`) keeps that behavior; the initial
 *   target is still authorized.
 */
export function createEnforcedFetch(
    base: FetchFn,
    policy: CapabilityPolicy,
): FetchFn {
    if (!policy.hasNet()) {
        return base;
    }

    const enforced: FetchFn = async (input, init) => {
        // Normalize to a canonical Request once (URL frozen to a stable string),
        // closing the TOCTOU window for every input shape — string, URL, or a
        // (possibly hostile) Request subclass with a mutating `url` getter.
        const reqInput = input instanceof URL ? input.href : input;
        const req = new Request(reqInput as RequestInfo, init);
        authorizeUrl(policy, new URL(req.url));

        if (req.redirect !== "follow") {
            // Caller manages redirects themselves; a single up-front check of the
            // frozen target is enough (a `manual` follow re-enters this wrapper).
            return base(req);
        }
        return followWithAuthorization(base, req, policy);
    };

    return enforced;
}

/** Maximum redirect hops to follow before giving up (matches common runtimes). */
const MAX_REDIRECTS = 20;

/**
 * Follow redirects manually, authorizing the destination of every 3xx hop, so a
 * redirect to a policy-denied host is refused *before* the connection to it is
 * opened — even on a runtime whose native `fetch` bypasses the JS socket patch.
 */
async function followWithAuthorization(
    base: FetchFn,
    initial: Request,
    policy: CapabilityPolicy,
): Promise<Response> {
    let current = initial;
    for (let hop = 0; ; hop++) {
        if (hop > MAX_REDIRECTS) {
            throw new Error(
                "omni: exceeded the maximum redirect count while enforcing " +
                    "the `net` policy",
            );
        }
        // Clone for the send so `current` keeps an un-consumed body available to
        // build the next hop (307/308 preserve the body).
        const res = await base(current.clone(), { redirect: "manual" });
        const location =
            res.status >= 300 && res.status < 400
                ? res.headers.get("location")
                : null;
        if (!location) {
            return res;
        }
        const next = new URL(location, current.url);
        authorizeUrl(policy, next);
        current = redirectedRequest(current, res.status, next);
    }
}

/**
 * Build the next request for a redirect hop following the Fetch spec's method
 * rules: a `303`, or a `301`/`302` on a non-`GET`/`HEAD` request, switches to
 * `GET` and drops the body; `307`/`308` (and same-method `301`/`302`) preserve
 * the method and body.
 */
function redirectedRequest(prev: Request, status: number, next: URL): Request {
    const dropBody =
        status === 303 ||
        ((status === 301 || status === 302) &&
            prev.method !== "GET" &&
            prev.method !== "HEAD");
    if (dropBody) {
        return new Request(next.href, {
            method: "GET",
            headers: prev.headers,
            redirect: "manual",
        });
    }
    return new Request(next.href, prev);
}

/**
 * Authorize a concrete request `url` against the `net` policy, throwing a
 * {@link NetworkPolicyError} when it is denied.
 */
function authorizeUrl(policy: CapabilityPolicy, url: URL): void {
    const host = url.hostname;
    const port = requestPort(url);
    if (!policy.checkNet(host, port)) {
        throw new NetworkPolicyError(host, port);
    }
}

/**
 * The `{ host, port }` a URL-shaped connect target names (`WebSocket`,
 * `http2.connect`, …), or `null` when it cannot be parsed as an absolute URL —
 * in which case the caller lets it proceed to the runtime/OS floor rather than
 * guessing. The port defaults to the protocol's well-known port, matching
 * {@link requestPort}, so a `wss://h/` grant of `h:443` authorizes it.
 */
export function netTargetFromUrl(
    input: unknown,
): { host: string; port: number } | null {
    let url: URL;
    try {
        if (input instanceof URL) {
            url = input;
        } else if (typeof input === "string") {
            url = new URL(input);
        } else if (
            input &&
            typeof input === "object" &&
            typeof (input as { url?: unknown }).url === "string"
        ) {
            url = new URL((input as { url: string }).url);
        } else {
            return null;
        }
    } catch {
        return null;
    }
    return { host: url.hostname, port: requestPort(url) };
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
