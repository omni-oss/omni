import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { describe, expect, test } from "vitest";

import { importScript } from "./import";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES = join(__dirname, "__fixtures__");

const namedPath = join(FIXTURES, "named.mjs");
const defaultPath = join(FIXTURES, "default.mjs");
const factoryPath = join(FIXTURES, "factory.mjs");
const cjsPath = join(FIXTURES, "cjs.cjs");

describe("import", () => {
    describe("importScript() argument validation", () => {
        test("throws TypeError on an empty spec", async () => {
            await expect(importScript("")).rejects.toBeInstanceOf(TypeError);
        });

        test("throws TypeError on a non-string spec", async () => {
            // Force a runtime check that the type guard wouldn't normally
            // allow — covers the defensive `typeof` branch.
            await expect(
                importScript(undefined as unknown as string),
            ).rejects.toBeInstanceOf(TypeError);
        });
    });

    describe("importScript() specifier handling", () => {
        test("loads from an absolute filesystem path", async () => {
            const mod = await importScript<{
                greeting: string;
                value: number;
                add: (a: number, b: number) => number;
            }>(namedPath);

            expect(mod.greeting).toBe("hello");
            expect(mod.value).toBe(42);
            expect(mod.add(2, 3)).toBe(5);
        });

        test("loads from a file:// URL", async () => {
            const url = pathToFileURL(namedPath).href;
            const mod = await importScript<{ greeting: string }>(url);
            expect(mod.greeting).toBe("hello");
        });

        test("loads from a relative path resolved against cwd", async () => {
            // The loader resolves relative specs against process.cwd(),
            // so we have to express the path that way for the test to be
            // independent of where vitest was invoked from.
            const relSpec = `./${relative(process.cwd(), namedPath).replace(/\\/g, "/")}`;

            const mod = await importScript<{ value: number }>(relSpec);
            expect(mod.value).toBe(42);
        });

        test("passes bare specifiers through to the runtime resolver", async () => {
            // `node:path` is a built-in bare specifier the runtime always
            // resolves. This proves toImportSpecifier doesn't try to turn
            // it into a file URL.
            const mod =
                await importScript<typeof import("node:path")>("node:path");
            expect(typeof mod.join).toBe("function");
            expect(mod.join("a", "b")).toBe(join("a", "b"));
        });

        test("treats Windows-style absolute paths as paths, not URLs", async () => {
            // On non-Windows platforms there is no C:\ path, so we synthesize
            // the assertion: any absolute filesystem path that the OS knows
            // about must round-trip through importScript without being mistaken
            // for a URL with a one-letter scheme.
            const absolute = resolve(namedPath);
            const mod = await importScript<{ value: number }>(absolute);
            expect(mod.value).toBe(42);
        });

        test("strips Windows extended-length path prefix (\\\\?\\) before converting to a URL", async () => {
            // Rust's std::fs::canonicalize prepends \\?\ to paths on Windows.
            // importScript must strip this prefix so pathToFileURL produces a
            // valid file:// URL rather than file://%3F\…
            //
            // We prepend the prefix to the real fixture path; on non-Windows
            // platforms the prefix is stripped by our normalizer and the
            // remaining absolute path is resolved normally.
            const realPath = resolve(namedPath);
            // Simulate what Rust's canonicalize returns: \\?\<absolutePath>
            const withPrefix = `\\\\?\\${realPath}`;

            const mod = await importScript<{ value: number }>(withPrefix);
            expect(mod.value).toBe(42);
        });
    });

    describe("importScript() returns module exports as-is", () => {
        test("preserves named exports without normalization", async () => {
            const mod = await importScript<{
                greeting: string;
                value: number;
                add: (a: number, b: number) => number;
                default?: unknown;
            }>(namedPath);

            // Module namespace objects expose their named exports directly.
            expect(Object.keys(mod).sort()).toEqual(
                ["add", "greeting", "value"].sort(),
            );
            // No default normalization happened.
            expect(mod.default).toBeUndefined();
        });

        test("preserves the default export under `default`", async () => {
            const mod = await importScript<{
                default: { kind: string; id: number };
                meta: string;
            }>(defaultPath);

            expect(mod.default).toEqual({ kind: "default-export", id: 1 });
            expect(mod.meta).toBe("side-info");
        });

        test("does not invoke factory-style default exports", async () => {
            const mod = await importScript<{
                default: (opts: unknown) => unknown;
            }>(factoryPath);

            // The factory must be returned as a function — not called.
            expect(typeof mod.default).toBe("function");
            expect(mod.default({ ping: true })).toEqual({
                kind: "factory",
                opts: { ping: true },
            });
        });

        test("loads CommonJS modules via Node's ESM interop", async () => {
            const mod = await importScript<{
                default: { kind: string; id: number };
            }>(cjsPath);

            // Node surfaces `module.exports` as the `default` export.
            expect(mod.default).toEqual({ kind: "cjs", id: 7 });
        });
    });

    describe("importScript() module identity", () => {
        test("returns the same module instance for the same spec (URL cache)", async () => {
            const a = await importScript(namedPath);
            const b = await importScript(namedPath);
            // Dynamic import caches by resolved URL.
            expect(a).toBe(b);
        });

        test("returns the same module for path and equivalent file URL", async () => {
            const viaPath = await importScript(namedPath);
            const viaUrl = await importScript(pathToFileURL(namedPath).href);
            expect(viaPath).toBe(viaUrl);
        });
    });
});
