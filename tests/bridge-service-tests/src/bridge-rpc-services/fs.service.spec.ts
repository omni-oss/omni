import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ResponseStatusCode } from "@omni-oss/bridge-rpc-core";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { TEXT } from "@/helpers";
import { call, callWithBody } from "./helpers";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("bridge_rpc_services – /fs/* (omni_bridge_test_service)", {
    timeout: 30_000,
}, () => {
    /** Unique base directory for this test run, under the OS temp dir. */
    let testDir: string;

    beforeAll(async () => {
        testDir = join(tmpdir(), `bridge-fs-${randomUUID()}`);
        // Create the base test directory through the bridge so that
        // the setup itself exercises the service.
        const result = await call("/fs/create-directory", {
            path: testDir,
            options: { recursive: true },
        });
        if (!result.status.equals(ResponseStatusCode.SUCCESS)) {
            throw new Error(
                `Failed to create test directory: status ${result.status}`,
            );
        }
    });

    afterAll(async () => {
        // Best-effort cleanup via the bridge.
        try {
            await call("/fs/remove", {
                path: testDir,
                options: { recursive: true },
            });
        } catch {
            // Ignore – the OS temp cleaner will handle it eventually.
        }
    });

    // ---------------------------------------------------------------
    // /fs/create-directory
    // ---------------------------------------------------------------
    describe("/fs/create-directory", () => {
        it("creates a flat directory", async () => {
            const dir = join(testDir, "flat");
            const result = await call("/fs/create-directory", {
                path: dir,
                options: { recursive: false },
            });
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
        });

        it("creates nested directories when recursive=true", async () => {
            const dir = join(testDir, "nested", "a", "b", "c");
            const result = await call("/fs/create-directory", {
                path: dir,
                options: { recursive: true },
            });
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
        });
    });

    // ---------------------------------------------------------------
    // /fs/path-exists
    // ---------------------------------------------------------------
    describe("/fs/path-exists", () => {
        it("returns true for an existing directory", async () => {
            const result = await call<{ value: boolean }>("/fs/path-exists", {
                path: testDir,
            });
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.value).toBe(true);
        });

        it("returns false for a non-existent path", async () => {
            const result = await call<{ value: boolean }>("/fs/path-exists", {
                path: join(testDir, "does-not-exist-abc123"),
            });
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.value).toBe(false);
        });
    });

    // ---------------------------------------------------------------
    // /fs/write-string-to-file  /fs/read-file-as-string
    // ---------------------------------------------------------------
    describe("/fs/write-string-to-file and /fs/read-file-as-string", () => {
        it("round-trips UTF-8 text content", async () => {
            const file = join(testDir, "hello.txt");
            const content = "Hello, Bridge RPC! 🦀";

            const writeResult = await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode(content),
            );
            expect(writeResult.status).toEqual(ResponseStatusCode.SUCCESS);

            const readResult = await call("/fs/read-file-as-string", {
                path: file,
            });
            expect(readResult.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(TEXT.decode(readResult.body)).toBe(content);
        });

        it("overwrites existing file content", async () => {
            const file = join(testDir, "overwrite.txt");

            await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode("first version"),
            );

            await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode("second version"),
            );

            const readResult = await call("/fs/read-file-as-string", {
                path: file,
            });
            expect(TEXT.decode(readResult.body)).toBe("second version");
        });

        it("writes and reads an empty file", async () => {
            const file = join(testDir, "empty.txt");
            const writeResult = await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                new Uint8Array(0),
            );
            expect(writeResult.status).toEqual(ResponseStatusCode.SUCCESS);

            const readResult = await call("/fs/read-file-as-string", {
                path: file,
            });
            expect(readResult.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(readResult.body.byteLength).toBe(0);
        });
    });

    // ---------------------------------------------------------------
    // /fs/write-bytes-to-file  /fs/read-file-as-bytes
    // ---------------------------------------------------------------
    describe("/fs/write-bytes-to-file and /fs/read-file-as-bytes", () => {
        it("round-trips arbitrary binary data", async () => {
            const file = join(testDir, "binary.bin");
            // Include bytes that are invalid UTF-8 to confirm raw binary
            // handling works.
            const data = new Uint8Array([0x00, 0xff, 0xfe, 0x80, 0x7f, 0x01]);

            const writeResult = await callWithBody(
                "/fs/write-bytes-to-file",
                { path: file },
                data,
            );
            expect(writeResult.status).toEqual(ResponseStatusCode.SUCCESS);

            const readResult = await call("/fs/read-file-as-bytes", {
                path: file,
            });
            expect(readResult.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(Array.from(readResult.body)).toEqual(Array.from(data));
        });
    });

    // ---------------------------------------------------------------
    // /fs/append-string-to-file
    // ---------------------------------------------------------------
    describe("/fs/append-string-to-file", () => {
        it("appends content to an existing file", async () => {
            const file = join(testDir, "append.txt");

            await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode("line1\n"),
            );
            await callWithBody(
                "/fs/append-string-to-file",
                { path: file },
                TEXT.encode("line2\n"),
            );
            await callWithBody(
                "/fs/append-string-to-file",
                { path: file },
                TEXT.encode("line3\n"),
            );

            const readResult = await call("/fs/read-file-as-string", {
                path: file,
            });
            expect(TEXT.decode(readResult.body)).toBe("line1\nline2\nline3\n");
        });

        it("creates the file when it does not exist yet", async () => {
            const file = join(testDir, "append-new.txt");

            // File doesn't exist – append should create it.
            await callWithBody(
                "/fs/append-string-to-file",
                { path: file },
                TEXT.encode("created-by-append"),
            );

            const readResult = await call("/fs/read-file-as-string", {
                path: file,
            });
            expect(TEXT.decode(readResult.body)).toBe("created-by-append");
        });
    });

    // ---------------------------------------------------------------
    // /fs/stat
    // ---------------------------------------------------------------
    describe("/fs/stat", () => {
        it("returns file metadata for a regular file", async () => {
            const file = join(testDir, "stat-file.txt");
            await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode("stat test content"),
            );

            const result = await call<{
                is_file: boolean;
                is_directory: boolean;
                is_symbolic_link: boolean;
                size: number;
                mtime_ms: number | null;
            }>("/fs/stat", { path: file });

            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.is_file).toBe(true);
            expect(result.returns.is_directory).toBe(false);
            expect(result.returns.size).toBeGreaterThan(0);
        });

        it("returns directory metadata for a directory", async () => {
            const dir = join(testDir, "stat-dir");
            await call("/fs/create-directory", {
                path: dir,
                options: { recursive: false },
            });

            const result = await call<{
                is_file: boolean;
                is_directory: boolean;
                is_symbolic_link: boolean;
                size: number;
                mtime_ms: number | null;
            }>("/fs/stat", { path: dir });

            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.is_directory).toBe(true);
            expect(result.returns.is_file).toBe(false);
        });
    });

    // ---------------------------------------------------------------
    // /fs/is-file  /fs/is-directory  /fs/is-symbolic-link
    // ---------------------------------------------------------------
    describe("/fs/is-file, /fs/is-directory, /fs/is-symbolic-link", () => {
        it("is-file returns true for a regular file", async () => {
            const file = join(testDir, "is-file.txt");
            await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode("x"),
            );

            const result = await call<{ value: boolean }>("/fs/is-file", {
                path: file,
            });
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.value).toBe(true);
        });

        it("is-file returns false for a directory", async () => {
            const result = await call<{ value: boolean }>("/fs/is-file", {
                path: testDir,
            });
            expect(result.returns.value).toBe(false);
        });

        it("is-directory returns true for a directory", async () => {
            const result = await call<{ value: boolean }>("/fs/is-directory", {
                path: testDir,
            });
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.value).toBe(true);
        });

        it("is-directory returns false for a file", async () => {
            const file = join(testDir, "is-dir-file.txt");
            await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode("x"),
            );

            const result = await call<{ value: boolean }>("/fs/is-directory", {
                path: file,
            });
            expect(result.returns.value).toBe(false);
        });

        it("is-symbolic-link returns false for a regular file", async () => {
            const file = join(testDir, "not-symlink.txt");
            await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode("x"),
            );

            const result = await call<{ value: boolean }>(
                "/fs/is-symbolic-link",
                { path: file },
            );
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.value).toBe(false);
        });
    });

    // ---------------------------------------------------------------
    // /fs/read-directory
    // ---------------------------------------------------------------
    describe("/fs/read-directory", () => {
        it("returns the names of entries inside a directory", async () => {
            const dir = join(testDir, "readdir");
            await call("/fs/create-directory", {
                path: dir,
                options: { recursive: false },
            });

            const files = ["alpha.txt", "beta.txt", "gamma.txt"];
            await Promise.all(
                files.map((name) =>
                    callWithBody(
                        "/fs/write-string-to-file",
                        { path: join(dir, name) },
                        TEXT.encode(name),
                    ),
                ),
            );

            const result = await call<{ entries: string[] }>(
                "/fs/read-directory",
                { path: dir },
            );
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.entries.sort()).toEqual(files.sort());
        });

        it("returns an empty array for an empty directory", async () => {
            const dir = join(testDir, "empty-dir");
            await call("/fs/create-directory", {
                path: dir,
                options: { recursive: false },
            });

            const result = await call<{ entries: string[] }>(
                "/fs/read-directory",
                { path: dir },
            );
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(result.returns.entries).toEqual([]);
        });
    });

    // ---------------------------------------------------------------
    // /fs/rename
    // ---------------------------------------------------------------
    describe("/fs/rename", () => {
        it("moves a file to a new path", async () => {
            const src = join(testDir, "rename-src.txt");
            const dst = join(testDir, "rename-dst.txt");

            await callWithBody(
                "/fs/write-string-to-file",
                { path: src },
                TEXT.encode("rename-me"),
            );

            const renameResult = await call("/fs/rename", {
                old_path: src,
                new_path: dst,
            });
            expect(renameResult.status).toEqual(ResponseStatusCode.SUCCESS);

            // Source should be gone.
            const srcExists = await call<{ value: boolean }>(
                "/fs/path-exists",
                { path: src },
            );
            expect(srcExists.returns.value).toBe(false);

            // Destination should exist with original content.
            const readResult = await call("/fs/read-file-as-string", {
                path: dst,
            });
            expect(TEXT.decode(readResult.body)).toBe("rename-me");
        });
    });

    // ---------------------------------------------------------------
    // /fs/copy
    // ---------------------------------------------------------------
    describe("/fs/copy", () => {
        it("copies a file to a new path", async () => {
            const src = join(testDir, "copy-src.txt");
            const dst = join(testDir, "copy-dst.txt");

            await callWithBody(
                "/fs/write-string-to-file",
                { path: src },
                TEXT.encode("copy-content"),
            );

            const copyResult = await call("/fs/copy", {
                src,
                dest: dst,
                options: { overwrite: true, recursive: false },
            });
            expect(copyResult.status).toEqual(ResponseStatusCode.SUCCESS);

            // Both source and destination should exist.
            const srcExists = await call<{ value: boolean }>(
                "/fs/path-exists",
                { path: src },
            );
            expect(srcExists.returns.value).toBe(true);

            const readResult = await call("/fs/read-file-as-string", {
                path: dst,
            });
            expect(TEXT.decode(readResult.body)).toBe("copy-content");
        });
    });

    // ---------------------------------------------------------------
    // /fs/remove
    // ---------------------------------------------------------------
    describe("/fs/remove", () => {
        it("deletes a single file", async () => {
            const file = join(testDir, "to-delete.txt");
            await callWithBody(
                "/fs/write-string-to-file",
                { path: file },
                TEXT.encode("bye"),
            );

            const removeResult = await call("/fs/remove", {
                path: file,
                options: { recursive: false },
            });
            expect(removeResult.status).toEqual(ResponseStatusCode.SUCCESS);

            const existsResult = await call<{ value: boolean }>(
                "/fs/path-exists",
                { path: file },
            );
            expect(existsResult.returns.value).toBe(false);
        });

        it("deletes a directory tree when recursive=true", async () => {
            const dir = join(testDir, "tree-to-delete");
            await call("/fs/create-directory", {
                path: join(dir, "sub"),
                options: { recursive: true },
            });
            await callWithBody(
                "/fs/write-string-to-file",
                { path: join(dir, "sub", "file.txt") },
                TEXT.encode("x"),
            );

            const removeResult = await call("/fs/remove", {
                path: dir,
                options: { recursive: true },
            });
            expect(removeResult.status).toEqual(ResponseStatusCode.SUCCESS);

            const existsResult = await call<{ value: boolean }>(
                "/fs/path-exists",
                { path: dir },
            );
            expect(existsResult.returns.value).toBe(false);
        });

        it("is a no-op for a non-existent path", async () => {
            const result = await call("/fs/remove", {
                path: join(testDir, "ghost-file-xyz"),
                options: { recursive: false },
            });
            expect(result.status).toEqual(ResponseStatusCode.SUCCESS);
        });
    });
});
