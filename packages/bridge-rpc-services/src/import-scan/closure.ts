// The runtime-authoritative static import-closure computer.
//
// It NEVER reimplements module resolution and NEVER executes the scanned code.
// It (1) statically extracts specifiers with a lexer, then (2) resolves each by
// calling the *runtime's own* resolve API (injected as `resolve`), following
// the returned real paths recursively. First-party (relative/workspace) files
// are walked; `node_modules` packages are treated as grant BOUNDARIES (record
// the package root, do not walk their internals) so the closure stays bounded.
//
// Guards:
//   - `visited`           — canonical file paths already walked (cycle guard).
//   - `visitedGenerators` — generator entries already scanned, so a nested
//                           run-generator over an already-scanned generator is
//                           not re-walked.
//
// The lexer (`es-module-lexer`) is a zero-dependency WASM tokenizer that never
// executes code; it is comment- and string-safe and TypeScript-tolerant, so an
// `import()` mentioned in prose or a string never yields a false specifier (the
// failure mode of a naive regex). A non-literal dynamic `import(expr)` surfaces
// as a diagnostic rather than being silently missed.

import { readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { init as lexerInit, parse as lexerParse } from "es-module-lexer";

/**
 * Resolve `specifier` as imported from `parentUrl` (a `file:` URL), using the
 * runtime's own resolver. Returns a URL string (typically `file:`), or `null`
 * when the specifier does not resolve to a loadable module (e.g. a `node:`
 * builtin). Throws when the specifier is genuinely unresolvable.
 */
export type ResolveFn = (
    specifier: string,
    parentUrl: string,
) => Promise<string | null> | string | null;

export interface ClosureRequest {
    entries: string[];
    resolve: ResolveFn;
}

export interface ClosureResult {
    /** Canonical first-party files that must be readable (sorted). */
    closure: string[];
    /** Canonical `node_modules` package roots to grant as boundaries (sorted). */
    packageRoots: string[];
    visitedFiles: number;
    visitedGenerators: number;
    /** Non-fatal notes (unresolved specifiers, non-literal dynamic imports). */
    diagnostics: string[];
}

function canon(p: string): string {
    try {
        return realpathSync.native(p);
    } catch {
        return path.resolve(p);
    }
}

function isExternalSpecifier(spec: string): boolean {
    // http(s):, npm:, jsr:, data:, node: — not local files to grant/walk. A
    // Windows drive path (`C:\…`) is NOT external despite matching `x:`.
    return /^[a-z][a-z0-9+\-.]*:/i.test(spec) && !/^[a-zA-Z]:[\\/]/.test(spec);
}

function packageRootOf(fileAbs: string): string | null {
    // If the resolved path lives under a `node_modules`, return the package
    // root (`node_modules/<pkg>` or `node_modules/@scope/pkg`); else null.
    const parts = fileAbs.split(/[\\/]/);
    const nm = parts.lastIndexOf("node_modules");
    if (nm < 0 || nm + 1 >= parts.length) return null;
    let end = nm + 2;
    const first = parts[nm + 1];
    if (first?.startsWith("@") && nm + 2 < parts.length) end = nm + 3;
    return parts.slice(0, end).join(path.sep);
}

interface ExtractResult {
    specifiers: string[];
    nonLiteralDynamic: boolean;
}

export function extractSpecifiers(source: string): ExtractResult {
    const specifiers = new Set<string>();
    let nonLiteralDynamic = false;
    const [imports] = lexerParse(source);
    for (const imp of imports) {
        if (imp.d === -2) continue; // import.meta, not a module fetch
        const dynamic = imp.d >= 0;
        if (imp.n === undefined) {
            if (dynamic) nonLiteralDynamic = true; // import(expr) — unresolvable
            continue;
        }
        specifiers.add(imp.n);
    }
    return { specifiers: [...specifiers], nonLiteralDynamic };
}

/**
 * Compute the transitive first-party import closure of `entries` using the
 * runtime's own `resolve`. `node_modules` packages are recorded as bounded
 * grant roots rather than walked. A `gen:<path>` specifier stands in for a
 * nested run-generator: its target is scanned as a generator (deduplicated by
 * the `visitedGenerators` guard) instead of walked as a plain file.
 */
export async function computeClosure({
    entries,
    resolve,
}: ClosureRequest): Promise<ClosureResult> {
    await lexerInit;
    const visited = new Set<string>();
    const visitedGenerators = new Set<string>();
    const closure = new Set<string>();
    const packageRoots = new Set<string>();
    const diagnostics: string[] = [];

    async function scanGenerator(entryAbs: string): Promise<void> {
        const c = canon(entryAbs);
        if (visitedGenerators.has(c)) return;
        visitedGenerators.add(c);
        await walk(c);
    }

    async function walk(fileAbs: string): Promise<void> {
        const c = canon(fileAbs);
        if (visited.has(c)) return;
        visited.add(c);
        closure.add(c);

        let source: string;
        try {
            source = readFileSync(c, "utf8");
        } catch (e) {
            diagnostics.push(`read failed: ${c}: ${codeOf(e)}`);
            return;
        }

        const { specifiers, nonLiteralDynamic } = extractSpecifiers(source);
        if (nonLiteralDynamic) {
            diagnostics.push(
                `non-literal dynamic import() in ${c} — grant an explicit read root for its target`,
            );
        }

        const parentUrl = pathToFileURL(c).href;
        for (const spec of specifiers) {
            // A nested run-generator invocation, modelled by a `gen:<path>`
            // specifier. Scan the target AS A GENERATOR (deduplicated by
            // `visitedGenerators`) rather than walking it as a plain file.
            // Handled before `isExternalSpecifier`, which would otherwise treat
            // `gen:` as an external scheme.
            if (spec.startsWith("gen:")) {
                const target = spec.slice("gen:".length);
                let url: string | null;
                try {
                    url = await resolve(target, parentUrl);
                } catch (e) {
                    diagnostics.push(
                        `unresolved nested generator "${target}" from ${c}: ${codeOf(e)}`,
                    );
                    continue;
                }
                if (url?.startsWith("file:")) {
                    await scanGenerator(fileURLToPath(url));
                }
                continue;
            }
            if (isExternalSpecifier(spec)) continue;

            let url: string | null;
            try {
                url = await resolve(spec, parentUrl);
            } catch (e) {
                diagnostics.push(`unresolved "${spec}" from ${c}: ${codeOf(e)}`);
                continue;
            }
            if (!url?.startsWith("file:")) continue; // node: builtins etc.

            const resolved = canon(fileURLToPath(url));
            const pkg = packageRootOf(resolved);
            if (pkg) {
                packageRoots.add(pkg);
                closure.add(resolved); // grant the entry file too
            } else {
                await walk(resolved); // first-party: recurse
            }
        }
    }

    for (const entry of entries) {
        await scanGenerator(entry);
    }

    return {
        closure: [...closure].sort(),
        packageRoots: [...packageRoots].sort(),
        visitedFiles: visited.size,
        visitedGenerators: visitedGenerators.size,
        diagnostics,
    };
}

function codeOf(e: unknown): string {
    if (e && typeof e === "object" && "code" in e && e.code) {
        return String((e as { code: unknown }).code);
    }
    return e instanceof Error ? e.message : String(e);
}
