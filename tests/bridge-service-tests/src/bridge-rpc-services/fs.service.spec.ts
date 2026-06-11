import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BridgeRpcSystem } from "@omni-oss/bridge-rpc-system-interface";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("bridge_rpc_services – /fs/* (omni_bridge_test_service)", {
    timeout: 30_000,
}, () => {
    let system: BridgeRpcSystem;
    /** Unique base directory for this test run, under the OS temp dir. */
    let testDir: string;

    beforeAll(async () => {
        system = await BridgeRpcSystem.create(RsRpcClient);
        testDir = join(tmpdir(), `bridge-fs-${randomUUID()}`);
        // Create the base test directory through the bridge so that
        // the setup itself exercises the service.
        await system.fs.createDirectory(testDir, { recursive: true });
    });

    afterAll(async () => {
        // Best-effort cleanup via the bridge.
        try {
            await system.fs.remove(testDir, { recursive: true });
        } catch {
            // Ignore – the OS temp cleaner will handle it eventually.
        }
    });

    // ---------------------------------------------------------------
    // createDirectory
    // ---------------------------------------------------------------
    describe("createDirectory", () => {
        it("creates a flat directory", async () => {
            const dir = join(testDir, "flat");
            await system.fs.createDirectory(dir, { recursive: false });
            // Verify the directory was actually created.
            expect(await system.fs.pathExists(dir)).toBe(true);
            expect(await system.fs.isDirectory(dir)).toBe(true);
        });

        it("creates nested directories when recursive=true", async () => {
            const dir = join(testDir, "nested", "a", "b", "c");
            await system.fs.createDirectory(dir, { recursive: true });
            // Verify the full nested path was actually created.
            expect(await system.fs.pathExists(dir)).toBe(true);
            expect(await system.fs.isDirectory(dir)).toBe(true);
        });
    });

    // ---------------------------------------------------------------
    // pathExists
    // ---------------------------------------------------------------
    describe("pathExists", () => {
        it("returns true for an existing directory", async () => {
            const exists = await system.fs.pathExists(testDir);
            expect(exists).toBe(true);
        });

        it("returns false for a non-existent path", async () => {
            const exists = await system.fs.pathExists(
                join(testDir, "does-not-exist-abc123"),
            );
            expect(exists).toBe(false);
        });
    });

    // ---------------------------------------------------------------
    // writeStringToFile / readFileAsString
    // ---------------------------------------------------------------
    describe("writeStringToFile / readFileAsString", () => {
        it("round-trips UTF-8 text content", async () => {
            const file = join(testDir, "hello.txt");
            const content = "Hello, Bridge RPC! 🦀";

            await system.fs.writeStringToFile(file, content);
            const text = await system.fs.readFileAsString(file);
            expect(text).toBe(content);
        });

        it("overwrites existing file content", async () => {
            const file = join(testDir, "overwrite.txt");

            await system.fs.writeStringToFile(file, "first version");
            await system.fs.writeStringToFile(file, "second version");

            const text = await system.fs.readFileAsString(file);
            expect(text).toBe("second version");
        });

        it("writes and reads an empty file", async () => {
            const file = join(testDir, "empty.txt");
            await system.fs.writeStringToFile(file, "");
            const text = await system.fs.readFileAsString(file);
            expect(text).toBe("");
        });
    });

    // ---------------------------------------------------------------
    // writeBytesToFile / readFileAsBytes
    // ---------------------------------------------------------------
    describe("writeBytesToFile / readFileAsBytes", () => {
        it("round-trips arbitrary binary data", async () => {
            const file = join(testDir, "binary.bin");
            // Include bytes that are invalid UTF-8 to confirm raw binary
            // handling works.
            const data = new Uint8Array([0x00, 0xff, 0xfe, 0x80, 0x7f, 0x01]);

            await system.fs.writeBytesToFile(file, data);
            const bytes = await system.fs.readFileAsBytes(file);
            expect(Array.from(bytes)).toEqual(Array.from(data));
        });
    });

    // ---------------------------------------------------------------
    // appendStringToFile
    // ---------------------------------------------------------------
    describe("appendStringToFile", () => {
        it("appends content to an existing file", async () => {
            const file = join(testDir, "append.txt");

            await system.fs.writeStringToFile(file, "line1\n");
            await system.fs.appendStringToFile(file, "line2\n");
            await system.fs.appendStringToFile(file, "line3\n");

            const text = await system.fs.readFileAsString(file);
            expect(text).toBe("line1\nline2\nline3\n");
        });

        it("creates the file when it does not exist yet", async () => {
            const file = join(testDir, "append-new.txt");

            // File doesn't exist – append should create it.
            await system.fs.appendStringToFile(file, "created-by-append");

            const text = await system.fs.readFileAsString(file);
            expect(text).toBe("created-by-append");
        });
    });

    // ---------------------------------------------------------------
    // stat
    // ---------------------------------------------------------------
    describe("stat", () => {
        it("returns file metadata for a regular file", async () => {
            const file = join(testDir, "stat-file.txt");
            const content = "stat test content";
            await system.fs.writeStringToFile(file, content);

            const stat = await system.fs.stat(file);
            expect(stat.isFile()).toBe(true);
            expect(stat.isDirectory()).toBe(false);
            // All ASCII so byte length equals string length.
            expect(stat.size).toBe(content.length);
            expect(stat.mtime).toBeInstanceOf(Date);

            // Verify the written content is readable and intact.
            const readBack = await system.fs.readFileAsString(file);
            expect(readBack).toBe(content);
        });

        it("returns directory metadata for a directory", async () => {
            const dir = join(testDir, "stat-dir");
            await system.fs.createDirectory(dir, { recursive: false });

            const stat = await system.fs.stat(dir);
            expect(stat.isDirectory()).toBe(true);
            expect(stat.isFile()).toBe(false);
        });
    });

    // ---------------------------------------------------------------
    // isFile / isDirectory / isSymbolicLink
    // ---------------------------------------------------------------
    describe("isFile / isDirectory / isSymbolicLink", () => {
        it("isFile returns true for a regular file", async () => {
            const file = join(testDir, "is-file.txt");
            await system.fs.writeStringToFile(file, "x");

            expect(await system.fs.isFile(file)).toBe(true);
            // Verify the written content is readable and intact.
            expect(await system.fs.readFileAsString(file)).toBe("x");
        });

        it("isFile returns false for a directory", async () => {
            const result = await system.fs.isFile(testDir);
            expect(result).toBe(false);
        });

        it("isDirectory returns true for a directory", async () => {
            const result = await system.fs.isDirectory(testDir);
            expect(result).toBe(true);
        });

        it("isDirectory returns false for a file", async () => {
            const file = join(testDir, "is-dir-file.txt");
            await system.fs.writeStringToFile(file, "x");

            expect(await system.fs.isDirectory(file)).toBe(false);
            // Verify the written content is readable and intact.
            expect(await system.fs.readFileAsString(file)).toBe("x");
        });

        it("isSymbolicLink returns false for a regular file", async () => {
            const file = join(testDir, "not-symlink.txt");
            await system.fs.writeStringToFile(file, "x");

            expect(await system.fs.isSymbolicLink(file)).toBe(false);
            // Verify the written content is readable and intact.
            expect(await system.fs.readFileAsString(file)).toBe("x");
        });
    });

    // ---------------------------------------------------------------
    // readDirectory
    // ---------------------------------------------------------------
    describe("readDirectory", () => {
        it("returns the names of entries inside a directory", async () => {
            const dir = join(testDir, "readdir");
            await system.fs.createDirectory(dir, { recursive: false });

            const files = ["alpha.txt", "beta.txt", "gamma.txt"];
            await Promise.all(
                files.map((name) =>
                    system.fs.writeStringToFile(join(dir, name), name),
                ),
            );

            const entries = await system.fs.readDirectory(dir);
            expect(entries.sort()).toEqual(files.sort());

            // Verify each file's content is intact after listing.
            const contents = await Promise.all(
                files.map((name) =>
                    system.fs.readFileAsString(join(dir, name)),
                ),
            );
            for (const [i, content] of contents.entries()) {
                expect(content).toBe(files[i]);
            }
        });

        it("returns an empty array for an empty directory", async () => {
            const dir = join(testDir, "empty-dir");
            await system.fs.createDirectory(dir, { recursive: false });

            const entries = await system.fs.readDirectory(dir);
            expect(entries).toEqual([]);
        });
    });

    // ---------------------------------------------------------------
    // rename
    // ---------------------------------------------------------------
    describe("rename", () => {
        it("moves a file to a new path", async () => {
            const src = join(testDir, "rename-src.txt");
            const dst = join(testDir, "rename-dst.txt");

            await system.fs.writeStringToFile(src, "rename-me");
            await system.fs.rename(src, dst);

            // Source should be gone.
            const srcExists = await system.fs.pathExists(src);
            expect(srcExists).toBe(false);

            // Destination should exist with original content.
            const text = await system.fs.readFileAsString(dst);
            expect(text).toBe("rename-me");
        });
    });

    // ---------------------------------------------------------------
    // copy
    // ---------------------------------------------------------------
    describe("copy", () => {
        it("copies a file to a new path", async () => {
            const src = join(testDir, "copy-src.txt");
            const dst = join(testDir, "copy-dst.txt");

            await system.fs.writeStringToFile(src, "copy-content");
            await system.fs.copy(src, dst, { overwrite: true });

            // Both source and destination should exist.
            const srcExists = await system.fs.pathExists(src);
            expect(srcExists).toBe(true);

            // Destination must carry the copied content.
            const dstText = await system.fs.readFileAsString(dst);
            expect(dstText).toBe("copy-content");

            // Source must be unchanged by the copy.
            const srcText = await system.fs.readFileAsString(src);
            expect(srcText).toBe("copy-content");
        });
    });

    // ---------------------------------------------------------------
    // remove
    // ---------------------------------------------------------------
    describe("remove", () => {
        it("deletes a single file", async () => {
            const file = join(testDir, "to-delete.txt");
            await system.fs.writeStringToFile(file, "bye");

            // Verify the content is readable before removal.
            expect(await system.fs.readFileAsString(file)).toBe("bye");

            await system.fs.remove(file, { recursive: false });

            const exists = await system.fs.pathExists(file);
            expect(exists).toBe(false);
        });

        it("deletes a directory tree when recursive=true", async () => {
            const dir = join(testDir, "tree-to-delete");
            await system.fs.createDirectory(join(dir, "sub"), {
                recursive: true,
            });
            const nestedFile = join(dir, "sub", "file.txt");
            await system.fs.writeStringToFile(nestedFile, "x");

            // Verify the nested file is readable before tree removal.
            expect(await system.fs.readFileAsString(nestedFile)).toBe("x");

            await system.fs.remove(dir, { recursive: true });

            const exists = await system.fs.pathExists(dir);
            expect(exists).toBe(false);
        });

        it("is a no-op for a non-existent path", async () => {
            await expect(
                system.fs.remove(join(testDir, "ghost-file-xyz"), {
                    recursive: false,
                }),
            ).resolves.toBeUndefined();
        });
    });
});
