// Runtime-native resolvers for strategy C (lexer + the runtime's own resolve).
//
// Each returns a `ResolveFn` that never reimplements resolution: it delegates
// to the runtime's real resolver so conditional `exports`, subpaths, and
// package layout are honoured exactly as the runtime would at load time.
//
//   - Node: `import.meta.resolve` ignores its `parent` argument in stable Node
//     (it resolves relative to the *calling* module), so it cannot drive a
//     central scanner. Node therefore uses `createRequire(importer).resolve`
//     for bare specifiers and importer-relative path + extension/index probing
//     for relative/absolute ones — the cheap, exact part of resolution.
//   - Bun: `Bun.resolveSync(spec, dir)` takes the importer directory, so it
//     resolves both relative and bare specifiers (and tsconfig `paths`)
//     directly.

import { statSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { RUNTIME } from "@omni-oss/runtime-utils";
import type { ResolveFn } from "./closure";

const EXTS = [
    "",
    ".ts",
    ".tsx",
    ".mts",
    ".cts",
    ".js",
    ".jsx",
    ".mjs",
    ".cjs",
    ".json",
];

function isFile(p: string): boolean {
    try {
        return statSync(p).isFile();
    } catch {
        return false;
    }
}

function probe(base: string): string | null {
    for (const e of EXTS) if (e && isFile(base + e)) return base + e;
    if (isFile(base)) return base;
    for (const e of EXTS) {
        const p = path.join(base, `index${e}`);
        if (e && isFile(p)) return p;
    }
    return null;
}

function isRelativeOrAbsolute(spec: string): boolean {
    return (
        spec.startsWith("./") || spec.startsWith("../") || path.isAbsolute(spec)
    );
}

const nodeResolve: ResolveFn = (spec, parentUrl) => {
    const parentPath = fileURLToPath(parentUrl);
    if (isRelativeOrAbsolute(spec)) {
        const abs = path.resolve(path.dirname(parentPath), spec);
        const hit = probe(abs);
        if (!hit) {
            throw Object.assign(new Error(`not found: ${spec}`), {
                code: "ENOENT",
            });
        }
        return pathToFileURL(hit).href;
    }
    // Bare specifier -> real `node_modules` resolution from the importer.
    //
    // NOTE: Node's resolver does NOT honour tsconfig `paths`/`baseUrl` aliases,
    // so an alias like `@shared/*` will not resolve here. Rather than silently
    // miss such a target, `createRequire().resolve` throws, which the closure
    // walker records as an `unresolved "<spec>"` diagnostic. The remedy is a
    // `direct: true` read root for the alias target (or avoiding path aliases in
    // Node generators); Bun's resolver does honour tsconfig `paths`.
    return pathToFileURL(createRequire(parentPath).resolve(spec)).href;
};

interface BunResolver {
    resolveSync(spec: string, dir: string): string;
}

function bunGlobal(): BunResolver | undefined {
    return (globalThis as { Bun?: BunResolver }).Bun;
}

const bunResolve: ResolveFn = (spec, parentUrl) => {
    const bun = bunGlobal();
    if (!bun) {
        throw new Error("Bun.resolveSync is unavailable");
    }
    const parentDir = path.dirname(fileURLToPath(parentUrl));
    const resolved = bun.resolveSync(spec, parentDir);
    // `Bun.resolveSync` returns an absolute path (or a `node:` builtin name).
    if (isRelativeOrAbsolute(resolved) || path.isAbsolute(resolved)) {
        return pathToFileURL(resolved).href;
    }
    return null;
};

/** The strategy-C resolver for the current runtime (Node or Bun). */
export function makeResolver(): ResolveFn {
    return RUNTIME === "bun" ? bunResolve : nodeResolve;
}
