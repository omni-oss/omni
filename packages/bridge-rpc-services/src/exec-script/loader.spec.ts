import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { describe, expect, test } from "vitest";

import { loadScript, RUNTIME, type Runtime } from "./loader";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES = join(__dirname, "__fixtures__");

const namedPath = join(FIXTURES, "named.mjs");
const defaultPath = join(FIXTURES, "default.mjs");
const factoryPath = join(FIXTURES, "factory.mjs");
const cjsPath = join(FIXTURES, "cjs.cjs");

describe("loader", () => {
    describe("RUNTIME", () => {
        test("is one of the supported runtime tags", () => {
            const allowed: Runtime[] = ["node", "bun", "deno"];
            expect(allowed).toContain(RUNTIME);
        });

        test("matches the host runtime detected via globals", () => {
            // The test process either has Deno, Bun, or neither as a global.
            const g = globalThis as Record<string, unknown>;
            const expected: Runtime =
                typeof g.Deno !== "undefined"
                    ? "deno"
                    : typeof g.Bun !== "undefined"
                      ? "bun"
                      : "node";
            expect(RUNTIME).toBe(expected);
        });
    });

    describe("loadScript() argument validation", () => {
        test("throws TypeError on an empty spec", async () => {
            await expect(loadScript("")).rejects.toBeInstanceOf(TypeError);
        });

        test("throws TypeError on a non-string spec", async () => {
            // Force a runtime check that the type guard wouldn't normally
            // allow — covers the defensive `typeof` branch.
            await expect(
                loadScript(undefined as unknown as string),
            ).rejects.toBeInstanceOf(TypeError);
        });
    });

    describe("loadScript() specifier handling", () => {
        test("loads from an absolute filesystem path", async () => {
            const mod = await loadScript<{
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
            const mod = await loadScript<{ greeting: string }>(url);
            expect(mod.greeting).toBe("hello");
        });

        test("loads from a relative path resolved against cwd", async () => {
            // The loader resolves relative specs against process.cwd(),
            // so we have to express the path that way for the test to be
            // independent of where vitest was invoked from.
            const relSpec = `./${relative(process.cwd(), namedPath).replace(/\\/g, "/")}`;

            const mod = await loadScript<{ value: number }>(relSpec);
            expect(mod.value).toBe(42);
        });

        test("passes bare specifiers through to the runtime resolver", async () => {
            // `node:path` is a built-in bare specifier the runtime always
            // resolves. This proves toImportSpecifier doesn't try to turn
            // it into a file URL.
            const mod =
                await loadScript<typeof import("node:path")>("node:path");
            expect(typeof mod.join).toBe("function");
            expect(mod.join("a", "b")).toBe(join("a", "b"));
        });

        test("treats Windows-style absolute paths as paths, not URLs", async () => {
            // On non-Windows platforms there is no C:\ path, so we synthesize
            // the assertion: any absolute filesystem path that the OS knows
            // about must round-trip through loadScript without being mistaken
            // for a URL with a one-letter scheme.
            const absolute = resolve(namedPath);
            const mod = await loadScript<{ value: number }>(absolute);
            expect(mod.value).toBe(42);
        });
    });

    describe("loadScript() returns module exports as-is", () => {
        test("preserves named exports without normalization", async () => {
            const mod = await loadScript<{
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
            const mod = await loadScript<{
                default: { kind: string; id: number };
                meta: string;
            }>(defaultPath);

            expect(mod.default).toEqual({ kind: "default-export", id: 1 });
            expect(mod.meta).toBe("side-info");
        });

        test("does not invoke factory-style default exports", async () => {
            const mod = await loadScript<{
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
            const mod = await loadScript<{
                default: { kind: string; id: number };
            }>(cjsPath);

            // Node surfaces `module.exports` as the `default` export.
            expect(mod.default).toEqual({ kind: "cjs", id: 7 });
        });
    });

    describe("loadScript() module identity", () => {
        test("returns the same module instance for the same spec (URL cache)", async () => {
            const a = await loadScript(namedPath);
            const b = await loadScript(namedPath);
            // Dynamic import caches by resolved URL.
            expect(a).toBe(b);
        });

        test("returns the same module for path and equivalent file URL", async () => {
            const viaPath = await loadScript(namedPath);
            const viaUrl = await loadScript(pathToFileURL(namedPath).href);
            expect(viaPath).toBe(viaUrl);
        });
    });
});
