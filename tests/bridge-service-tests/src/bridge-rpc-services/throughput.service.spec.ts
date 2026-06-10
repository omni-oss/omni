/**
 * Throughput and protocol-stability tests for `bridge_rpc_services`.
 *
 * These tests exercise the bridge protocol under load by firing many
 * concurrent and sequential requests at the `omni_bridge_test_service`
 * child process.  The primary goal is to verify that the framing layer
 * never silently corrupts, drops, or misroutes messages – not to assert
 * hard latency thresholds.
 *
 * Timing information is logged at the `info` level so it is visible in
 * verbose test runs without causing flaky failures in slow CI environments.
 */
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { SerializableValue } from "@omni-oss/bridge-rpc-core";
import { ResponseStatusCode } from "@omni-oss/bridge-rpc-core";
import { readBody } from "@omni-oss/bridge-rpc-utils/body";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { TEXT } from "@/helpers";

// ---------------------------------------------------------------------------
// Shared helpers (duplicated from proc/fs specs to keep each file
// independently runnable)
// ---------------------------------------------------------------------------

async function procCall<T = unknown>(
    path: string,
    params?: Record<string, unknown>,
): Promise<{ status: ResponseStatusCode; returns: T }> {
    const req = await RsRpcClient.request(path);
    const active = await req.start(
        params ? { parameters: params as SerializableValue } : undefined,
    );
    const response = await active.end().then((r) => r.wait());
    await readBody(response);
    return {
        status: response.status,
        returns: (response.headers as { returns?: T } | undefined)
            ?.returns as T,
    };
}

async function fsCall<T = unknown>(
    servicePath: string,
    params: Record<string, unknown>,
): Promise<{ status: ResponseStatusCode; returns: T; body: Uint8Array }> {
    const req = await RsRpcClient.request(servicePath);
    const active = await req.start({
        parameters: params as SerializableValue,
    });
    const response = await active.end().then((r) => r.wait());
    const body = await readBody(response);
    return {
        status: response.status,
        returns: (response.headers as { returns?: T } | undefined)
            ?.returns as T,
        body,
    };
}

async function fsCallWithBody<T = unknown>(
    servicePath: string,
    params: Record<string, unknown>,
    bodyData: Uint8Array,
): Promise<{ status: ResponseStatusCode; returns: T; body: Uint8Array }> {
    const req = await RsRpcClient.request(servicePath);
    const active = await req.start({
        parameters: params as SerializableValue,
    });
    await active.writeBodyChunk(bodyData);
    const response = await active.end().then((r) => r.wait());
    const body = await readBody(response);
    return {
        status: response.status,
        returns: (response.headers as { returns?: T } | undefined)
            ?.returns as T,
        body,
    };
}

/** Log a human-readable throughput summary without failing the test. */
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

describe("bridge_rpc_services – throughput (omni_bridge_test_service)", {
    timeout: 120_000,
}, () => {
    // -------------------------------------------------------------------
    // Proc-service throughput
    // -------------------------------------------------------------------
    describe("proc services", () => {
        it("handles 100 concurrent /proc/snapshot requests without errors", async () => {
            const COUNT = 100;
            const start = performance.now();

            const results = await Promise.all(
                Array.from({ length: COUNT }, () =>
                    procCall<{
                        current_dir: string;
                        args: string[];
                        env: Record<string, string>;
                    }>("/proc/snapshot"),
                ),
            );

            const elapsed = performance.now() - start;
            logThroughput("100 concurrent /proc/snapshot", COUNT, elapsed);

            expect(results).toHaveLength(COUNT);
            for (const r of results) {
                expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
                expect(typeof r.returns.current_dir).toBe("string");
                expect(r.returns.current_dir.length).toBeGreaterThan(0);
            }
        });

        it("handles 500 sequential /proc/snapshot requests without errors", async () => {
            const COUNT = 500;
            const start = performance.now();

            for (let i = 0; i < COUNT; i++) {
                const result = await procCall<{
                    current_dir: string;
                    args: string[];
                    env: Record<string, string>;
                }>("/proc/snapshot");
                expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            }

            const elapsed = performance.now() - start;
            logThroughput("500 sequential /proc/snapshot", COUNT, elapsed);
        });

        it("handles 200 concurrent mixed proc requests without message misrouting", async () => {
            // Mix different services in a single concurrent wave to
            // confirm the framing layer correctly routes responses to
            // the matching request IDs.
            const tasks = [
                ...Array.from({ length: 70 }, () =>
                    procCall<{ current_dir: string }>("/proc/current-dir"),
                ),
                ...Array.from({ length: 70 }, () =>
                    procCall<{ args: string[] }>("/proc/args"),
                ),
                ...Array.from({ length: 60 }, () =>
                    procCall<{ env: Record<string, string> }>("/proc/env"),
                ),
            ];

            const start = performance.now();
            const results = await Promise.all(tasks);
            const elapsed = performance.now() - start;
            logThroughput("200 concurrent mixed proc", 200, elapsed);

            // All must succeed with the correct shape.
            for (const r of results.slice(0, 70)) {
                expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
                expect(
                    typeof (r.returns as { current_dir: string }).current_dir,
                ).toBe("string");
            }
            for (const r of results.slice(70, 140)) {
                expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
                expect(
                    Array.isArray((r.returns as { args: string[] }).args),
                ).toBe(true);
            }
            for (const r of results.slice(140, 200)) {
                expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
                expect(
                    typeof (r.returns as { env: Record<string, string> }).env,
                ).toBe("object");
            }
        });
    });

    // -------------------------------------------------------------------
    // FS-service throughput
    // -------------------------------------------------------------------
    describe("fs services", () => {
        let throughputDir: string;

        beforeAll(async () => {
            throughputDir = join(tmpdir(), `bridge-throughput-${randomUUID()}`);
            await fsCall("/fs/create-directory", {
                path: throughputDir,
                options: { recursive: true },
            });
        });

        afterAll(async () => {
            try {
                await fsCall("/fs/remove", {
                    path: throughputDir,
                    options: { recursive: true },
                });
            } catch {
                // Best-effort cleanup.
            }
        });

        it("handles 50 concurrent file write + read pairs without corruption", async () => {
            const COUNT = 50;
            const start = performance.now();

            // Write 50 files concurrently.
            await Promise.all(
                Array.from({ length: COUNT }, (_, i) => {
                    const content = `file-${i}: ${"x".repeat(512)}`;
                    return fsCallWithBody(
                        "/fs/write-string-to-file",
                        {
                            path: join(throughputDir, `concurrent-${i}.txt`),
                        },
                        TEXT.encode(content),
                    );
                }),
            );

            // Read all 50 back concurrently and verify content integrity.
            const readResults = await Promise.all(
                Array.from({ length: COUNT }, (_, i) =>
                    fsCall("/fs/read-file-as-string", {
                        path: join(throughputDir, `concurrent-${i}.txt`),
                    }),
                ),
            );

            const elapsed = performance.now() - start;
            logThroughput("50 concurrent write+read", COUNT * 2, elapsed);

            for (const [i, r] of readResults.entries()) {
                expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
                const text = TEXT.decode(r.body);
                expect(text).toContain(`file-${i}:`);
                // Content length must match (no truncation or merging).
                const expectedLength = `file-${i}: ${"x".repeat(512)}`.length;
                expect(text.length).toBe(expectedLength);
            }
        });

        it("handles a 1 MB file write and read without body truncation", async () => {
            const file = join(throughputDir, "large-1mb.txt");
            // 1 MiB of ASCII content (reproducible – not random – so we
            // can cheaply verify round-trip integrity without hashing).
            const CHUNK = "ABCDEFGHIJKLMNOP"; // 16 bytes
            const REPEAT = (1024 * 1024) / CHUNK.length; // 65 536
            const content = CHUNK.repeat(REPEAT);

            expect(content.length).toBe(1024 * 1024);

            const start = performance.now();

            const writeResult = await fsCallWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode(content),
            );
            expect(writeResult.status).toEqual(ResponseStatusCode.SUCCESS);

            const readResult = await fsCall("/fs/read-file-as-string", {
                path: file,
            });
            const elapsed = performance.now() - start;
            logThroughput("1 MB write+read", 2, elapsed);

            expect(readResult.status).toEqual(ResponseStatusCode.SUCCESS);
            const received = TEXT.decode(readResult.body);
            expect(received.length).toBe(content.length);
            expect(received).toBe(content);
        });

        it("handles a 4 MB file write and read split across multiple body chunks", async () => {
            const file = join(throughputDir, "large-4mb.txt");
            const CHUNK = "0123456789ABCDEF"; // 16 bytes
            const REPEAT = (4 * 1024 * 1024) / CHUNK.length;
            const content = CHUNK.repeat(REPEAT);

            expect(content.length).toBe(4 * 1024 * 1024);

            // Split the payload into 64 KB write chunks to exercise the
            // multi-chunk body path on the bridge transport.
            const WRITE_CHUNK_SIZE = 64 * 1024;
            const req = await RsRpcClient.request("/fs/write-string-to-file");
            const active = await req.start({
                parameters: { path: file } as SerializableValue,
            });

            const encoded = TEXT.encode(content);
            for (
                let offset = 0;
                offset < encoded.byteLength;
                offset += WRITE_CHUNK_SIZE
            ) {
                await active.writeBodyChunk(
                    encoded.slice(offset, offset + WRITE_CHUNK_SIZE),
                );
            }
            const writeResponse = await active.end().then((r) => r.wait());
            await readBody(writeResponse);
            expect(writeResponse.status).toEqual(ResponseStatusCode.SUCCESS);

            const readResult = await fsCall("/fs/read-file-as-bytes", {
                path: file,
            });
            expect(readResult.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(readResult.body.byteLength).toBe(encoded.byteLength);
            // Spot-check first, middle, and last bytes.
            expect(readResult.body[0]).toBe(encoded[0]);
            const mid = Math.floor(encoded.byteLength / 2);
            expect(readResult.body[mid]).toBe(encoded[mid]);
            expect(readResult.body.at(-1)).toBe(encoded.at(-1));
        });

        it("handles 100 concurrent /fs/path-exists queries without misrouting", async () => {
            // Pre-create some files so we get a mix of true/false results.
            const EXISTING = 50;
            const existingFiles = Array.from({ length: EXISTING }, (_, i) =>
                join(throughputDir, `exists-check-${i}.txt`),
            );

            await Promise.all(
                existingFiles.map((f) =>
                    fsCallWithBody(
                        "/fs/write-string-to-file",
                        { path: f },
                        TEXT.encode("x"),
                    ),
                ),
            );

            const missingFiles = Array.from({ length: 50 }, (_, i) =>
                join(throughputDir, `no-such-file-${i}-${randomUUID()}`),
            );

            const queries = [
                ...existingFiles.map((f) =>
                    fsCall<{ value: boolean }>("/fs/path-exists", {
                        path: f,
                    }),
                ),
                ...missingFiles.map((f) =>
                    fsCall<{ value: boolean }>("/fs/path-exists", {
                        path: f,
                    }),
                ),
            ];

            const start = performance.now();
            const results = await Promise.all(queries);
            const elapsed = performance.now() - start;
            logThroughput("100 concurrent /fs/path-exists", 100, elapsed);

            // First 50 → existing files
            for (const r of results.slice(0, EXISTING)) {
                expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
                expect(r.returns.value).toBe(true);
            }
            // Next 50 → missing files
            for (const r of results.slice(EXISTING, 100)) {
                expect(r.status).toEqual(ResponseStatusCode.SUCCESS);
                expect(r.returns.value).toBe(false);
            }
        });

        it("handles 200 sequential write-stat-read cycles without state corruption", async () => {
            const COUNT = 200;
            const start = performance.now();

            for (let i = 0; i < COUNT; i++) {
                const file = join(throughputDir, `seq-${i}.txt`);
                const content = `seq-entry-${i}`;

                await fsCallWithBody(
                    "/fs/write-string-to-file",
                    { path: file },
                    TEXT.encode(content),
                );

                const stat = await fsCall<{
                    is_file: boolean;
                    size: number;
                }>("/fs/stat", { path: file });
                expect(stat.returns.is_file).toBe(true);
                expect(stat.returns.size).toBeGreaterThan(0);

                const read = await fsCall("/fs/read-file-as-string", {
                    path: file,
                });
                expect(TEXT.decode(read.body)).toBe(content);
            }

            const elapsed = performance.now() - start;
            logThroughput("200 sequential write-stat-read", COUNT * 3, elapsed);
        });
    });

    // -------------------------------------------------------------------
    // Ping stability under concurrent load
    // -------------------------------------------------------------------
    describe("bridge ping", () => {
        it("responds to pings while concurrent requests are in flight", async () => {
            // Saturate the bridge with proc calls while pinging, to
            // confirm that the control-plane (ping/pong) is not blocked
            // by data-plane traffic.
            const LOAD = 50;
            const loadPromise = Promise.all(
                Array.from({ length: LOAD }, () => procCall("/proc/snapshot")),
            );

            // Fire three pings in parallel with the load.
            const pings = await Promise.all([
                RsRpc.ping(5_000),
                RsRpc.ping(5_000),
                RsRpc.ping(5_000),
            ]);

            await loadPromise;

            for (const pong of pings) {
                expect(pong).toBe(true);
            }
        });
    });
});
