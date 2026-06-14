import fs from "node:fs";
import os from "node:os";
import path, { resolve } from "node:path";
import type {
    FileStat,
    FileSystem,
    Process,
    System,
} from "@omni-oss/system-interface";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { InMemoryCwdSystem } from "./in-memory-cwd-system";

// ──────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────
//
const TMP_DIR = path.join(
    fs.realpathSync(os.tmpdir()),
    `test-${crypto.randomUUID()}`,
);
const HOST_CWD = TMP_DIR;
const OUTPUT_DIR = path.join(TMP_DIR, "output");

const STAT_MARKER = { __marker: "stat" } as unknown as FileStat;

/**
 * Builds a fully-mocked inner `System`. Every file-system method is a spy so
 * the wrapper's path-resolution behaviour can be asserted on the arguments it
 * forwards. `proc.setCurrentDir` is spied so we can prove it is never called.
 */
function makeInnerSystem() {
    const fs = {
        readFileAsString: vi.fn().mockResolvedValue("contents"),
        readFileAsBytes: vi.fn().mockResolvedValue(new Uint8Array([1, 2, 3])),
        writeStringToFile: vi.fn().mockResolvedValue(undefined),
        writeBytesToFile: vi.fn().mockResolvedValue(undefined),
        pathExists: vi.fn().mockResolvedValue(true),
        createDirectory: vi.fn().mockResolvedValue(undefined),
        readDirectory: vi.fn().mockResolvedValue(["a", "b"]),
        remove: vi.fn().mockResolvedValue(undefined),
        rename: vi.fn().mockResolvedValue(undefined),
        stat: vi.fn().mockResolvedValue(STAT_MARKER),
        isFile: vi.fn().mockResolvedValue(true),
        isDirectory: vi.fn().mockResolvedValue(false),
        isSymbolicLink: vi.fn().mockResolvedValue(false),
        copy: vi.fn().mockResolvedValue(undefined),
        appendStringToFile: vi.fn().mockResolvedValue(undefined),
    } satisfies Record<keyof FileSystem, ReturnType<typeof vi.fn>>;

    const ARGS = ["node", "script.js"] as const;
    const ENV = { PATH: "/usr/bin", HOME: "/home/me" };

    const proc = {
        currentDir: vi.fn().mockReturnValue(HOST_CWD),
        setCurrentDir: vi.fn().mockResolvedValue(undefined),
        args: vi.fn().mockReturnValue(ARGS),
        env: vi.fn().mockReturnValue(ENV),
    } satisfies Record<keyof Process, ReturnType<typeof vi.fn>>;

    const system = {
        fs: fs as unknown as FileSystem,
        proc: proc as unknown as Process,
    } satisfies System;

    return { system, fs, proc, ARGS, ENV };
}

// ──────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────

describe("InMemoryCwdSystem", () => {
    let inner: ReturnType<typeof makeInnerSystem>;
    let sys: InMemoryCwdSystem;

    beforeEach(() => {
        inner = makeInnerSystem();
        sys = InMemoryCwdSystem.wrap(inner.system, OUTPUT_DIR);
    });

    describe("current directory", () => {
        test("starts at the provided initial (output) dir", () => {
            expect(sys.proc.currentDir()).toBe(OUTPUT_DIR);
        });

        test("does not read the host process current dir", () => {
            sys.proc.currentDir();
            expect(inner.proc.currentDir).not.toHaveBeenCalled();
        });

        test("setCurrentDir updates the in-memory dir only", async () => {
            const newDir = path.join(sys.proc.currentDir(), "sub");
            await sys.proc.setCurrentDir(newDir);
            expect(sys.proc.currentDir()).toBe(newDir);
        });

        test("setCurrentDir never touches the host process", async () => {
            await sys.proc.setCurrentDir("/somewhere/else");
            expect(inner.proc.setCurrentDir).not.toHaveBeenCalled();
        });

        test("setCurrentDir resolves relative dirs against the current dir", async () => {
            await sys.proc.setCurrentDir("nested/dir");
            expect(sys.proc.currentDir()).toBe(
                resolve(OUTPUT_DIR, "nested/dir"),
            );
        });

        test("setCurrentDir handles `..` relative navigation", async () => {
            await sys.proc.setCurrentDir("nested/dir");
            await sys.proc.setCurrentDir("../sibling");
            expect(sys.proc.currentDir()).toBe(
                resolve(OUTPUT_DIR, "nested/sibling"),
            );
        });

        test("setCurrentDir accepts absolute dirs verbatim", async () => {
            const newDir = path.join(HOST_CWD, "abs", "path");
            await sys.proc.setCurrentDir(newDir);
            expect(sys.proc.currentDir()).toBe(newDir);
        });
    });

    describe("process passthrough", () => {
        test("args() delegates to the inner process", () => {
            expect(sys.proc.args()).toBe(inner.ARGS);
            expect(inner.proc.args).toHaveBeenCalledTimes(1);
        });

        test("env() delegates to the inner process", () => {
            expect(sys.proc.env()).toBe(inner.ENV);
            expect(inner.proc.env).toHaveBeenCalledTimes(1);
        });
    });

    describe("path resolution for file-system ops", () => {
        test("resolves relative paths against the initial dir", async () => {
            await sys.fs.readFileAsString("foo.txt");
            expect(inner.fs.readFileAsString).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "foo.txt"),
            );
        });

        test("leaves absolute paths unchanged", async () => {
            const dir = path.join(HOST_CWD, "data");
            await sys.fs.readFileAsString(dir);
            expect(inner.fs.readFileAsString).toHaveBeenCalledWith(dir);
        });

        test("follows the in-memory dir after setCurrentDir", async () => {
            await sys.proc.setCurrentDir("sub");
            await sys.fs.readFileAsString("foo.txt");
            expect(inner.fs.readFileAsString).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "sub", "foo.txt"),
            );
        });

        test("readFileAsBytes resolves the path", async () => {
            await sys.fs.readFileAsBytes("data.bin");
            expect(inner.fs.readFileAsBytes).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "data.bin"),
            );
        });

        test("writeStringToFile resolves the path and forwards content", async () => {
            await sys.fs.writeStringToFile("out.txt", "hello");
            expect(inner.fs.writeStringToFile).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "out.txt"),
                "hello",
            );
        });

        test("writeBytesToFile resolves the path and forwards content", async () => {
            const bytes = new Uint8Array([4, 5, 6]);
            await sys.fs.writeBytesToFile("out.bin", bytes);
            expect(inner.fs.writeBytesToFile).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "out.bin"),
                bytes,
            );
        });

        test("pathExists resolves the path", async () => {
            await sys.fs.pathExists("maybe");
            expect(inner.fs.pathExists).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "maybe"),
            );
        });

        test("createDirectory resolves the path and forwards options", async () => {
            await sys.fs.createDirectory("nested", { recursive: true });
            expect(inner.fs.createDirectory).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "nested"),
                { recursive: true },
            );
        });

        test("readDirectory resolves the path", async () => {
            await sys.fs.readDirectory("dir");
            expect(inner.fs.readDirectory).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "dir"),
            );
        });

        test("remove resolves the path and forwards options", async () => {
            await sys.fs.remove("dir", { recursive: true });
            expect(inner.fs.remove).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "dir"),
                { recursive: true },
            );
        });

        test("rename resolves both old and new paths", async () => {
            await sys.fs.rename("old.txt", "new.txt");
            expect(inner.fs.rename).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "old.txt"),
                resolve(OUTPUT_DIR, "new.txt"),
            );
        });

        test("stat resolves the path and returns the inner result", async () => {
            const result = await sys.fs.stat("file");
            expect(inner.fs.stat).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "file"),
            );
            expect(result).toBe(STAT_MARKER);
        });

        test("isFile resolves the path", async () => {
            await sys.fs.isFile("file");
            expect(inner.fs.isFile).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "file"),
            );
        });

        test("isDirectory resolves the path", async () => {
            await sys.fs.isDirectory("dir");
            expect(inner.fs.isDirectory).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "dir"),
            );
        });

        test("isSymbolicLink resolves the path", async () => {
            await sys.fs.isSymbolicLink("link");
            expect(inner.fs.isSymbolicLink).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "link"),
            );
        });

        test("copy resolves both src and dest and forwards options", async () => {
            await sys.fs.copy("a.txt", "b.txt", { overwrite: true });
            expect(inner.fs.copy).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "a.txt"),
                resolve(OUTPUT_DIR, "b.txt"),
                { overwrite: true },
            );
        });

        test("appendStringToFile resolves the path and forwards content", async () => {
            await sys.fs.appendStringToFile("log.txt", "line\n");
            expect(inner.fs.appendStringToFile).toHaveBeenCalledWith(
                resolve(OUTPUT_DIR, "log.txt"),
                "line\n",
            );
        });

        test("propagates the inner file-system return value", async () => {
            await expect(sys.fs.readFileAsString("foo.txt")).resolves.toBe(
                "contents",
            );
        });
    });
});
