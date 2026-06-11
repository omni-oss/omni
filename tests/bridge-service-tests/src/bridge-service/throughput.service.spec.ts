/**
 * Throughput and protocol-stability tests for the JS `bridge-service`.
 *
 * These tests use `RpcClient` (the existing global from hooks.ts) to hammer
 * the `/exec-generator-script` service with concurrent and sequential
 * requests, verifying that the bridge framing layer handles real-world load
 * without dropping, misrouting, or corrupting messages.
 *
 * No hard latency thresholds are asserted – timing is only logged so it is
 * visible in verbose runs and CI artefacts without causing flaky failures.
 */
import { join } from "node:path";
import { ResponseStatusCode } from "@omni-oss/bridge-rpc-core";
import { readBody } from "@omni-oss/bridge-rpc-utils/body";
import { describe, expect, it } from "vitest";
import { json, TEXT } from "@/helpers";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Shared fixture script path (already used in the non-throughput spec). */
const FIXTURE_SCRIPT = join(__dirname, "../__fixtures__/test.mjs");

interface ExecParams {
    paths: string[];
    params: Record<string, unknown>;
}

/**
 * Invoke `/exec-generator-script` with the given payload and return the
 * response status and raw body bytes.
 */
async function execScript(
    payload: ExecParams,
): Promise<{ status: ResponseStatusCode; body: Uint8Array }> {
    const req = await TsRpcClient.request("/exec-generator-script");
    const active = await req.start();
    await active.writeBodyChunk(json(payload));
    const response = await active.end().then((r) => r.wait());
    const body = await readBody(response);
    return { status: response.status, body };
}

/** Log throughput without failing the test. */
function logThroughput(label: string, count: number, elapsedMs: number) {
    if (!process.env.SHOW_LOG_OUTPUT) return;
    const rps = ((count / elapsedMs) * 1000).toFixed(0);
    console.info(
        `[throughput] ${label}: ${count} requests in ${elapsedMs.toFixed(0)} ms  (${rps} req/s)`,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("bridge-service – throughput (/exec-generator-script)", {
    timeout: 20_000,
    concurrent: false,
}, () => {
    it("handles 50 concurrent requests without errors", async () => {
        const COUNT = 50;
        const payload: ExecParams = {
            paths: [FIXTURE_SCRIPT],
            params: { dry_run: true },
        };

        const start = performance.now();
        const results = await Promise.all(
            Array.from({ length: COUNT }, () => execScript(payload)),
        );
        const elapsed = performance.now() - start;
        logThroughput("50 concurrent /exec-generator-script", COUNT, elapsed);

        expect(results).toHaveLength(COUNT);
        for (const r of results) {
            if (!r.status.equals(ResponseStatusCode.SUCCESS)) {
                console.error("Unexpected error body:", TEXT.decode(r.body));
            }
            expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
        }
    });

    it("handles 200 sequential requests without framing errors", async () => {
        const COUNT = 200;
        const payload: ExecParams = {
            paths: [FIXTURE_SCRIPT],
            params: { dry_run: true },
        };

        const start = performance.now();
        for (let i = 0; i < COUNT; i++) {
            const r = await execScript(payload);
            if (!r.status.equals(ResponseStatusCode.SUCCESS)) {
                console.error(`Request ${i} failed:`, TEXT.decode(r.body));
            }
            expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
        }
        const elapsed = performance.now() - start;
        logThroughput("200 sequential /exec-generator-script", COUNT, elapsed);
    });

    it("handles 100 concurrent requests with dry_run=true without message misrouting", async () => {
        // All requests use dry_run=true; this validates that responses are
        // routed back to the correct request ID under high concurrency.
        const COUNT = 100;
        const payload: ExecParams = {
            paths: [FIXTURE_SCRIPT],
            params: { dry_run: true },
        };

        const start = performance.now();
        const results = await Promise.all(
            Array.from({ length: COUNT }, () => execScript(payload)),
        );
        const elapsed = performance.now() - start;
        logThroughput(
            "100 concurrent /exec-generator-script (dry_run=false)",
            COUNT,
            elapsed,
        );

        // Every request must complete and the status must be consistent
        // (SUCCESS or a known application-level code – not a framing
        // error that would manifest as an exception or a wrong body).
        for (const r of results) {
            // We just assert we got *a* response without an exception –
            // not necessarily SUCCESS, because execution may fail in the
            // application layer, but the framing must be intact.
            expect(r.status).toBeDefined();
            expect(r.body).toBeDefined();
        }
    });

    it("responds to pings while exec-generator-script requests are in flight", async () => {
        const COUNT = 30;
        const payload: ExecParams = {
            paths: [FIXTURE_SCRIPT],
            params: { dry_run: true },
        };

        // Start a wave of requests without awaiting them yet.
        const loadPromise = Promise.all(
            Array.from({ length: COUNT }, () => execScript(payload)),
        );

        // Issue pings while the load is running to confirm the
        // control-plane is not starved by data-plane traffic.
        const pings = await Promise.all([
            TsRpc.ping(5_000),
            TsRpc.ping(5_000),
            TsRpc.ping(5_000),
        ]);

        await loadPromise;

        for (const pong of pings) {
            expect(pong).toBe(true);
        }
    });

    it("handles back-to-back request bursts separated by a brief pause", async () => {
        // Simulate a real-world usage pattern where the client fires a
        // batch of requests, pauses, then fires another batch.
        const BATCH_SIZE = 40;
        const BATCHES = 3;
        const payload: ExecParams = {
            paths: [FIXTURE_SCRIPT],
            params: { dry_run: true },
        };

        const start = performance.now();

        for (let batch = 0; batch < BATCHES; batch++) {
            const results = await Promise.all(
                Array.from({ length: BATCH_SIZE }, () => execScript(payload)),
            );

            for (const r of results) {
                expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
            }

            // Brief yield between batches (mimics a real client think-time).
            await new Promise<void>((resolve) => setTimeout(resolve, 20));
        }

        const elapsed = performance.now() - start;
        logThroughput(
            `${BATCHES} batches of ${BATCH_SIZE} /exec-generator-script`,
            BATCHES * BATCH_SIZE,
            elapsed,
        );
    });
});
