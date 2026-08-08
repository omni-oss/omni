import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { describe, expect, test } from "vitest";
import { computeClosure, type ResolveFn } from "./closure";
import { makeResolver } from "./resolvers";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES = join(__dirname, "__fixtures__");
const resolve = makeResolver();

function endsWith(paths: string[], tail: string): boolean {
    const norm = tail.replace(/\//g, "");
    return paths.some((p) => p.replace(/[\\/]/g, "").endsWith(norm));
}

describe("computeClosure (strategy C)", () => {
    test("terminates on an a<->b import cycle, including both files once", async () => {
        const entry = join(FIXTURES, "cycle-a.ts");
        const result = await computeClosure({ entries: [entry], resolve });

        expect(result.visitedFiles).toBe(2);
        expect(endsWith(result.closure, "cycle-a.ts")).toBe(true);
        expect(endsWith(result.closure, "cycle-b.ts")).toBe(true);
        expect(result.diagnostics).toEqual([]);
    });

    test("scans a nested `gen:` generator once, deduped across entries", async () => {
        const genA = join(FIXTURES, "gen-a.ts");
        const genB = join(FIXTURES, "gen-b.ts");

        // gen-b nests gen-a via a `gen:` specifier; passing both as entries must
        // not re-scan gen-a (the visited-generators guard).
        const result = await computeClosure({
            entries: [genA, genB],
            resolve,
        });

        expect(result.visitedGenerators).toBe(2);
        expect(endsWith(result.closure, "gen-a.ts")).toBe(true);
        expect(endsWith(result.closure, "gen-b.ts")).toBe(true);
        expect(result.diagnostics).toEqual([]);
    });

    test("records a node_modules package root as a bounded boundary, not walked", async () => {
        // An injected resolver stands in for the runtime resolver, mapping the
        // two bare specifiers to conditional-exports/subpath targets under a
        // shared package root. The walker must record the ONE package root and
        // grant the entry files, without recursing into the package.
        const pkgDir = join(FIXTURES, "node_modules", "exports-fixture");
        const injected: ResolveFn = (spec) => {
            if (spec === "exports-fixture") {
                return pathToFileURL(join(pkgDir, "main.mjs")).href;
            }
            if (spec === "exports-fixture/sub") {
                return pathToFileURL(join(pkgDir, "sub.mjs")).href;
            }
            throw Object.assign(new Error(`not found: ${spec}`), {
                code: "ENOENT",
            });
        };

        const entry = join(FIXTURES, "entry-bare.ts");
        const result = await computeClosure({
            entries: [entry],
            resolve: injected,
        });

        // Exactly one package root recorded (dedup across the root + subpath).
        expect(result.packageRoots.length).toBe(1);
        expect(endsWith(result.packageRoots, "node_modulesexports-fixture")).toBe(
            true,
        );
        // The resolved package entry files are granted but the package is not
        // walked (only the real entry file was read → visitedFiles === 1).
        expect(result.visitedFiles).toBe(1);
        expect(endsWith(result.closure, "main.mjs")).toBe(true);
        expect(endsWith(result.closure, "sub.mjs")).toBe(true);
    });

    test("flags a non-literal dynamic import as a diagnostic", async () => {
        const entry = join(FIXTURES, "dynamic.ts");
        const result = await computeClosure({ entries: [entry], resolve });
        expect(
            result.diagnostics.some((d) =>
                d.includes("non-literal dynamic import()"),
            ),
        ).toBe(true);
    });
});
