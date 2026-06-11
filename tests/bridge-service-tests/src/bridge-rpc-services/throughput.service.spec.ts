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
import { BridgeRpcSystem } from "@omni-oss/bridge-rpc-system-interface";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { TEXT } from "@/helpers";

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
    timeout: 20_000,
    concurrent: false,
}, () => {
    let system: BridgeRpcSystem;
    let throughputDir: string;

    beforeAll(async () => {
        system = await BridgeRpcSystem.create(RsRpcClient);
        throughputDir = join(tmpdir(), `bridge-throughput-${randomUUID()}`);
        await system.fs.createDirectory(throughputDir, { recursive: true });
    });

    afterAll(async () => {
        try {
            await system.fs.remove(throughputDir, { recursive: true });
        } catch {
            // Best-effort cleanup.
        }
    });

    // -------------------------------------------------------------------
    // Proc-service throughput
    // -------------------------------------------------------------------
    describe("proc services", () => {
        it("handles 100 concurrent refreshSnapshot calls without errors", async () => {
            const COUNT = 100;
            const start = performance.now();

            await Promise.all(
                Array.from({ length: COUNT }, () =>
                    system.proc.refreshSnapshot(),
                ),
            );

            const elapsed = performance.now() - start;
            logThroughput("100 concurrent refreshSnapshot", COUNT, elapsed);

            expect(typeof system.proc.currentDir()).toBe("string");
            expect(system.proc.currentDir().length).toBeGreaterThan(0);
            expect(Array.isArray(system.proc.args())).toBe(true);
            expect(Object.keys(system.proc.env()).length).toBeGreaterThan(0);
        });

        it("handles 500 sequential refreshSnapshot calls without errors", async () => {
            const COUNT = 500;
            const start = performance.now();

            for (let i = 0; i < COUNT; i++) {
                await system.proc.refreshSnapshot();
                expect(system.proc.currentDir().length).toBeGreaterThan(0);
            }

            const elapsed = performance.now() - start;
            logThroughput("500 sequential refreshSnapshot", COUNT, elapsed);
        });

        it("handles 200 concurrent mixed proc+fs requests without message misrouting", async () => {
            // Mix snapshot refreshes with different FS queries in a single
            // concurrent wave to confirm the framing layer correctly routes
            // responses to the matching request IDs.
            const refreshTasks = Array.from({ length: 70 }, () =>
                system.proc.refreshSnapshot(),
            );
            const pathExistsTasks = Array.from({ length: 70 }, () =>
                system.fs.pathExists(throughputDir),
            );
            const isDirTasks = Array.from({ length: 60 }, () =>
                system.fs.isDirectory(throughputDir),
            );

            const start = performance.now();
            const [, pathExistsResults, isDirResults] = await Promise.all([
                Promise.all(refreshTasks),
                Promise.all(pathExistsTasks),
                Promise.all(isDirTasks),
            ]);
            const elapsed = performance.now() - start;
            logThroughput("200 concurrent mixed proc+fs", 200, elapsed);

            // All pathExists calls should correctly identify the existing dir.
            for (const r of pathExistsResults) {
                expect(r).toBe(true);
            }
            // All isDirectory calls should correctly identify the directory.
            for (const r of isDirResults) {
                expect(r).toBe(true);
            }
        });
    });

    // -------------------------------------------------------------------
    // FS-service throughput
    // -------------------------------------------------------------------
    describe("fs services", () => {
        it("handles 50 concurrent file write + read pairs without corruption", async () => {
            const COUNT = 50;
            const start = performance.now();

            // Write 50 files concurrently.
            await Promise.all(
                Array.from({ length: COUNT }, (_, i) => {
                    const content = `file-${i}: ${"x".repeat(512)}`;
                    return system.fs.writeStringToFile(
                        join(throughputDir, `concurrent-${i}.txt`),
                        content,
                    );
                }),
            );

            // Read all 50 back concurrently and verify content integrity.
            const texts = await Promise.all(
                Array.from({ length: COUNT }, (_, i) =>
                    system.fs.readFileAsString(
                        join(throughputDir, `concurrent-${i}.txt`),
                    ),
                ),
            );

            const elapsed = performance.now() - start;
            logThroughput("50 concurrent write+read", COUNT * 2, elapsed);

            for (const [i, text] of texts.entries()) {
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
            await system.fs.writeStringToFile(file, content);
            const received = await system.fs.readFileAsString(file);
            const elapsed = performance.now() - start;
            logThroughput("1 MB write+read", 2, elapsed);

            expect(received.length).toBe(content.length);
            expect(received).toBe(content);
        });

        it("handles a 4 MB file write and read split across multiple body chunks", async () => {
            const file = join(throughputDir, "large-4mb.txt");
            const CHUNK = "0123456789ABCDEF"; // 16 bytes
            const REPEAT = (4 * 1024 * 1024) / CHUNK.length;
            const content = CHUNK.repeat(REPEAT);

            expect(content.length).toBe(4 * 1024 * 1024);

            const encoded = TEXT.encode(content);
            // writeBytesToFile internally splits the payload into maxChunkSize
            // (64 KiB by default) chunks, exercising the multi-chunk body
            // path on the bridge transport.
            const start = performance.now();
            await system.fs.writeBytesToFile(file, encoded);
            const received = await system.fs.readFileAsBytes(file);
            const elapsed = performance.now() - start;
            logThroughput("4 MB multi-chunk write+read", 2, elapsed);

            expect(received.byteLength).toBe(encoded.byteLength);
            // Spot-check first, middle, and last bytes.
            expect(received[0]).toBe(encoded[0]);
            const mid = Math.floor(encoded.byteLength / 2);
            expect(received[mid]).toBe(encoded[mid]);
            expect(received.at(-1)).toBe(encoded.at(-1));
        });

        it("handles 100 concurrent /fs/path-exists queries without misrouting", async () => {
            // Pre-create some files so we get a mix of true/false results.
            const EXISTING = 50;
            const existingFiles = Array.from({ length: EXISTING }, (_, i) =>
                join(throughputDir, `exists-check-${i}.txt`),
            );

            await Promise.all(
                existingFiles.map((f) => system.fs.writeStringToFile(f, "x")),
            );

            const missingFiles = Array.from({ length: 50 }, (_, i) =>
                join(throughputDir, `no-such-file-${i}-${randomUUID()}`),
            );

            const start = performance.now();
            const results = await Promise.all([
                ...existingFiles.map((f) => system.fs.pathExists(f)),
                ...missingFiles.map((f) => system.fs.pathExists(f)),
            ]);
            const elapsed = performance.now() - start;
            logThroughput("100 concurrent pathExists", 100, elapsed);

            // First 50 → existing files
            for (const r of results.slice(0, EXISTING)) {
                expect(r).toBe(true);
            }
            // Next 50 → missing files
            for (const r of results.slice(EXISTING, 100)) {
                expect(r).toBe(false);
            }

            // Read all existing files back concurrently to confirm their
            // content was written and is still intact.
            const contents = await Promise.all(
                existingFiles.map((f) => system.fs.readFileAsString(f)),
            );
            for (const content of contents) {
                expect(content).toBe("x");
            }
        });

        it("handles 200 sequential write-stat-read-delete-exists cycles without state corruption", async () => {
            const COUNT = 200;
            const start = performance.now();

            for (let i = 0; i < COUNT; i++) {
                const file = join(throughputDir, `seq-${i}.txt`);
                const content = `seq-entry-${i}`;

                await system.fs.writeStringToFile(file, content);

                const stat = await system.fs.stat(file);
                expect(stat.isFile()).toBe(true);
                expect(stat.size).toBeGreaterThan(0);

                const text = await system.fs.readFileAsString(file);
                expect(text).toBe(content);
                await system.fs.remove(file);
                expect(await system.fs.pathExists(file)).toBe(false);
            }

            const elapsed = performance.now() - start;
            logThroughput(
                "200 sequential write-stat-read-delete-exists",
                COUNT * 3,
                elapsed,
            );
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
                Array.from({ length: LOAD }, () =>
                    system.proc.refreshSnapshot(),
                ),
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
