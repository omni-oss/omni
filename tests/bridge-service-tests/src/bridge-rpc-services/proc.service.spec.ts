import { tmpdir } from "node:os";
import { join } from "node:path";
import { BridgeRpcSystem } from "@omni-oss/bridge-rpc-system-interface";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("bridge_rpc_services – /proc/* (omni_bridge_test_service)", {
    timeout: 15_000,
}, () => {
    let system: BridgeRpcSystem;

    beforeAll(async () => {
        system = await BridgeRpcSystem.create(RsRpcClient);
    });

    // ---------------------------------------------------------------
    // Snapshot (populated via BridgeRpcSystem.create)
    // ---------------------------------------------------------------
    describe("snapshot", () => {
        it("currentDir() returns a non-empty string", () => {
            expect(typeof system.proc.currentDir()).toBe("string");
            expect(system.proc.currentDir().length).toBeGreaterThan(0);
        });

        it("args() returns an array of process arguments", () => {
            expect(Array.isArray(system.proc.args())).toBe(true);
        });

        it("env() returns a non-empty object", () => {
            const env = system.proc.env();
            expect(typeof env).toBe("object");
            expect(env).not.toBeNull();
            expect(Object.keys(env).length).toBeGreaterThan(0);
        });

        it("env values are all strings", () => {
            const env = system.proc.env();
            for (const key of env.keys())
                expect(
                    typeof env.get(key),
                    `env.get("${key}") should be a string`,
                ).toBe("string");
        });

        it("snapshot is consistent with a freshly created system", async () => {
            const fresh = await BridgeRpcSystem.create(RsRpcClient);
            // Both should reflect the same working directory.
            expect(system.proc.currentDir()).toBe(fresh.proc.currentDir());
        });
    });

    // ---------------------------------------------------------------
    // refreshSnapshot
    // ---------------------------------------------------------------
    describe("refreshSnapshot", () => {
        it("re-fetches the snapshot and preserves the current dir", async () => {
            const before = system.proc.currentDir();
            await system.proc.refreshSnapshot();
            expect(system.proc.currentDir()).toBe(before);
        });

        it("returns the same path on repeated refreshes", async () => {
            await system.proc.refreshSnapshot();
            const dir1 = system.proc.currentDir();
            await system.proc.refreshSnapshot();
            const dir2 = system.proc.currentDir();
            expect(dir1).toBe(dir2);
        });
    });

    // ---------------------------------------------------------------
    // setCurrentDir
    // ---------------------------------------------------------------
    describe("setCurrentDir", () => {
        let savedDir: string;
        const tempDir = join(
            tmpdir(),
            `omni-bridge-service-test-${Date.now()}-${crypto.randomUUID()}`,
        );

        beforeAll(async () => {
            // Capture the initial CWD so we can restore it after the
            // test (setCurrentDir is process-global in the child).
            savedDir = system.proc.currentDir();
            await system.fs.createDirectory(tempDir, { recursive: true });
            // Verify the temp directory was actually created.
            expect(await system.fs.isDirectory(tempDir)).toBe(true);
            if (process.env.SHOW_LOG_OUTPUT) {
                console.log(
                    `Saved initial CWD for setCurrentDir tests: ${savedDir}, created temp directory: ${tempDir}`,
                );
            }
        });

        afterAll(async () => {
            // Restore the original CWD so subsequent tests that rely
            // on relative paths are unaffected.
            await system.proc.setCurrentDir(savedDir);
        });

        it("changes the working directory and is reflected by currentDir()", async () => {
            await system.proc.setCurrentDir(tempDir);

            // Use a case-insensitive, partial comparison to handle
            // platforms where tmpdir() may resolve symlinks differently.
            const normalise = (p: string) => p.toLowerCase();
            const tmpLeaf =
                tempDir.split(/[/\\]/).filter(Boolean).at(-1)?.toLowerCase() ??
                "";
            expect(normalise(system.proc.currentDir())).toContain(tmpLeaf);
        });

        it("succeeds even when the directory is already the current directory", async () => {
            const currentDir = system.proc.currentDir();
            await expect(
                system.proc.setCurrentDir(currentDir),
            ).resolves.toBeUndefined();
        });
    });
});
