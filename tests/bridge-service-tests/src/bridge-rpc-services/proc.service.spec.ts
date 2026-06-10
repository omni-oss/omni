import { tmpdir } from "node:os";
import { join } from "node:path";
import { ResponseStatusCode } from "@omni-oss/bridge-rpc-core";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { call } from "./helpers";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("bridge_rpc_services – /proc/* (omni_bridge_test_service)", {
    timeout: 15_000,
}, () => {
    // ---------------------------------------------------------------
    // /proc/snapshot
    // ---------------------------------------------------------------
    describe("/proc/snapshot", () => {
        interface Snapshot {
            current_dir: string;
            args: string[];
            env: Record<string, string>;
        }

        it("returns a non-empty current_dir", async () => {
            const { status, returns } = await call<Snapshot>("/proc/snapshot");

            expect(status).toEqual(ResponseStatusCode.SUCCESS);
            expect(typeof returns.current_dir).toBe("string");
            expect(returns.current_dir.length).toBeGreaterThan(0);
        });

        it("returns args as an array", async () => {
            const { status, returns } = await call<Snapshot>("/proc/snapshot");

            expect(status).toEqual(ResponseStatusCode.SUCCESS);
            expect(Array.isArray(returns.args)).toBe(true);
        });

        it("returns env as a non-empty object", async () => {
            const { status, returns } = await call<Snapshot>("/proc/snapshot");

            expect(status).toEqual(ResponseStatusCode.SUCCESS);
            expect(typeof returns.env).toBe("object");
            expect(returns.env).not.toBeNull();
            expect(Object.keys(returns.env).length).toBeGreaterThan(0);
        });

        it("snapshot fields are consistent with individual service calls", async () => {
            const [snapshot, currentDir] = await Promise.all([
                call<Snapshot>("/proc/snapshot"),
                call<{ current_dir: string }>("/proc/current-dir"),
            ]);

            expect(snapshot.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(currentDir.status).toEqual(ResponseStatusCode.SUCCESS);
            // Both should reflect the same working directory.
            expect(snapshot.returns.current_dir).toBe(
                currentDir.returns.current_dir,
            );
        });
    });

    // ---------------------------------------------------------------
    // /proc/current-dir
    // ---------------------------------------------------------------
    describe("/proc/current-dir", () => {
        it("returns a non-empty string path", async () => {
            const { status, returns } = await call<{
                current_dir: string;
            }>("/proc/current-dir");

            expect(status).toEqual(ResponseStatusCode.SUCCESS);
            expect(typeof returns.current_dir).toBe("string");
            expect(returns.current_dir.length).toBeGreaterThan(0);
        });

        it("returns the same path on repeated calls", async () => {
            const [a, b] = await Promise.all([
                call<{ current_dir: string }>("/proc/current-dir"),
                call<{ current_dir: string }>("/proc/current-dir"),
            ]);

            expect(a.returns.current_dir).toBe(b.returns.current_dir);
        });
    });

    // ---------------------------------------------------------------
    // /proc/args
    // ---------------------------------------------------------------
    describe("/proc/args", () => {
        it("returns an array of process arguments", async () => {
            const { status, returns } = await call<{ args: string[] }>(
                "/proc/args",
            );

            expect(status).toEqual(ResponseStatusCode.SUCCESS);
            expect(Array.isArray(returns.args)).toBe(true);
        });
    });

    // ---------------------------------------------------------------
    // /proc/env
    // ---------------------------------------------------------------
    describe("/proc/env", () => {
        it("returns an object with at least one environment variable", async () => {
            const { status, returns } = await call<{
                env: Record<string, string>;
            }>("/proc/env");

            expect(status).toEqual(ResponseStatusCode.SUCCESS);
            expect(typeof returns.env).toBe("object");
            expect(returns.env).not.toBeNull();
            expect(Object.keys(returns.env).length).toBeGreaterThan(0);
        });

        it("env values are all strings", async () => {
            const { returns } = await call<{
                env: Record<string, string>;
            }>("/proc/env");

            for (const [key, value] of Object.entries(returns.env)) {
                expect(typeof value, `env[${key}] should be a string`).toBe(
                    "string",
                );
            }
        });

        it("snapshot env matches env service", async () => {
            const [snapshot, env] = await Promise.all([
                call<{
                    current_dir: string;
                    args: string[];
                    env: Record<string, string>;
                }>("/proc/snapshot"),
                call<{ env: Record<string, string> }>("/proc/env"),
            ]);

            // The env captured by both calls should share the same set of
            // keys (the child process env doesn't change between calls).
            const snapshotKeys = Object.keys(snapshot.returns.env).sort();
            const envKeys = Object.keys(env.returns.env).sort();
            expect(snapshotKeys).toEqual(envKeys);
        });
    });

    // ---------------------------------------------------------------
    // /proc/set-current-dir
    // ---------------------------------------------------------------
    describe("/proc/set-current-dir", () => {
        let savedDir: string;
        const tempDir = join(
            tmpdir(),
            `omni-bridge-service-test-${Date.now()}-${crypto.randomUUID()}`,
        );

        beforeAll(async () => {
            // Capture the initial CWD so we can restore it after the
            // test (set-current-dir is process-global in the child).
            const { returns: r1, status } = await call<{
                current_dir: string;
            }>("/proc/current-dir");

            if (status !== ResponseStatusCode.SUCCESS) {
                throw new Error(
                    `Failed to get initial current directory: status ${status.toString()}`,
                );
            }

            savedDir = r1.current_dir;

            const { returns: _r2, status: s2 } = await call(
                "/fs/create-directory",
                {
                    path: tempDir,
                    options: { recursive: true },
                },
            );

            if (s2 !== ResponseStatusCode.SUCCESS) {
                throw new Error(
                    `Failed to create temp directory for set-current-dir tests: status ${s2.toString()}`,
                );
            }
            if (process.env.SHOW_LOG_OUTPUT) {
                console.log(
                    `Saved initial CWD for set-current-dir tests: ${savedDir}, created temp directory: ${tempDir}`,
                );
            }
        });

        afterAll(async () => {
            // Restore the original CWD so subsequent tests that rely
            // on relative paths are unaffected.
            await call("/proc/set-current-dir", { dir: savedDir });
        });

        it("changes the working directory and is reflected by current-dir", async () => {
            const setResult = await call("/proc/set-current-dir", {
                dir: tempDir,
            });
            expect(setResult.status).toEqual(ResponseStatusCode.SUCCESS);

            const { returns } = await call<{ current_dir: string }>(
                "/proc/current-dir",
            );

            // Use a case-insensitive, partial comparison to handle
            // platforms where tmpdir() may resolve symlinks differently.
            const normalise = (p: string) => p.toLowerCase();
            const tmpLeaf =
                tempDir.split(/[/\\]/).filter(Boolean).at(-1)?.toLowerCase() ??
                "";
            expect(normalise(returns.current_dir)).toContain(tmpLeaf);
        });

        it("returns SUCCESS even when the directory is the current directory", async () => {
            const { returns: beforeReturns } = await call<{
                current_dir: string;
            }>("/proc/current-dir");

            const result = await call("/proc/set-current-dir", {
                dir: beforeReturns.current_dir,
            });
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
        });
    });
});
