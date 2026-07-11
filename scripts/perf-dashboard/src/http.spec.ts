import { describe, expect, it } from "vitest";
import { fetchWithRetry } from "./http";

/** A fetch that yields the given responses/errors in order (last repeats). */
function seqFetch(items: Array<Response | Error>): {
    fetchImpl: typeof fetch;
    calls: () => number;
} {
    let i = 0;
    let calls = 0;
    const fetchImpl = (async () => {
        calls += 1;
        const item = items[Math.min(i, items.length - 1)];
        i += 1;
        if (item instanceof Error) throw item;
        return item as Response;
    }) as unknown as typeof fetch;
    return { fetchImpl, calls: () => calls };
}

const res = (status: number) => new Response("", { status });

describe("fetchWithRetry", () => {
    it("returns immediately on success", async () => {
        const { fetchImpl, calls } = seqFetch([res(200)]);
        const r = await fetchWithRetry("u", {}, { fetchImpl, baseDelayMs: 0 });
        expect(r.status).toBe(200);
        expect(calls()).toBe(1);
    });

    it("retries retryable statuses then succeeds", async () => {
        const { fetchImpl, calls } = seqFetch([res(503), res(200)]);
        const r = await fetchWithRetry("u", {}, { fetchImpl, baseDelayMs: 0 });
        expect(r.status).toBe(200);
        expect(calls()).toBe(2);
    });

    it("retries network errors then succeeds", async () => {
        const { fetchImpl, calls } = seqFetch([new Error("boom"), res(200)]);
        const r = await fetchWithRetry("u", {}, { fetchImpl, baseDelayMs: 0 });
        expect(r.status).toBe(200);
        expect(calls()).toBe(2);
    });

    it("does not retry non-retryable statuses", async () => {
        const { fetchImpl, calls } = seqFetch([res(404)]);
        const r = await fetchWithRetry("u", {}, { fetchImpl, baseDelayMs: 0 });
        expect(r.status).toBe(404);
        expect(calls()).toBe(1);
    });

    it("gives up after maxAttempts, returning the last response", async () => {
        const { fetchImpl, calls } = seqFetch([res(503)]);
        const r = await fetchWithRetry(
            "u",
            {},
            { fetchImpl, baseDelayMs: 0, maxAttempts: 3 },
        );
        expect(r.status).toBe(503);
        expect(calls()).toBe(3);
    });

    it("throws when every attempt errors", async () => {
        const { fetchImpl, calls } = seqFetch([new Error("down")]);
        await expect(
            fetchWithRetry(
                "u",
                {},
                { fetchImpl, baseDelayMs: 0, maxAttempts: 2 },
            ),
        ).rejects.toThrow(/down/);
        expect(calls()).toBe(2);
    });

    it("invokes the onRetry hook", async () => {
        const { fetchImpl } = seqFetch([res(429), res(200)]);
        const seen: number[] = [];
        await fetchWithRetry(
            "u",
            {},
            {
                fetchImpl,
                baseDelayMs: 0,
                onRetry: ({ status }) => {
                    if (status !== undefined) seen.push(status);
                },
            },
        );
        expect(seen).toEqual([429]);
    });
});
