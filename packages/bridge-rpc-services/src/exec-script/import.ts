import { isAbsolute, resolve as resolvePath } from "node:path";
import { pathToFileURL } from "node:url";

// ──────────────────────────────────────────────────────────────────────────
// Specifier normalization
//
// Accepted script specs:
//   ./foo.ts | ../foo.ts          → relative path
//   /abs/foo.ts                   → POSIX absolute path
//   C:\scripts\foo.ts             → Windows absolute path
//   file:///…                     → file URL
//   https://… | npm:… | jsr:…     → URL specifiers (Deno/Bun)
//   @scope/pkg | pkg              → bare specifier, resolved by the runtime
//
// TypeScript policy: this loader does NOT transpile. Whether `.ts` works is
// the runtime's concern:
//   • Bun/Deno  → works natively.
//   • Node      → script authors must ship pre-compiled `.js`, or run Node
//                 with their own loader / --experimental-strip-types.
// ──────────────────────────────────────────────────────────────────────────
const URL_SCHEME_RE = /^[a-z][a-z0-9+\-.]*:/i;
const WIN_DRIVE_RE = /^[a-zA-Z]:[\\/]/;

function isUrlLike(spec: string): boolean {
    return URL_SCHEME_RE.test(spec) && !WIN_DRIVE_RE.test(spec);
}

function isRelative(spec: string): boolean {
    return spec.startsWith("./") || spec.startsWith("../");
}

function isBare(spec: string): boolean {
    return (
        !isUrlLike(spec) &&
        !isRelative(spec) &&
        !isAbsolute(spec) &&
        !WIN_DRIVE_RE.test(spec)
    );
}

function toImportSpecifier(spec: string): string {
    if (isUrlLike(spec) || isBare(spec)) return spec;
    // Relative or absolute filesystem path → file URL (Windows-safe).
    return pathToFileURL(resolvePath(spec)).href;
}

// ──────────────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────────────

/**
 * The shape of a module returned by `await import(...)`. The exact set of
 * exports depends on the script being loaded, so callers receive it as-is
 * and can narrow it themselves.
 */
export type ScriptModule = Record<string, unknown> & {
    default?: unknown;
};

/**
 * Load a script module by spec and return its exports as-is.
 *
 * No normalization, no factory invocation, no shape validation — callers get
 * the module namespace object directly from `await import(...)` and decide
 * what to do with it.
 *
 * @param spec  Path, URL, or bare specifier.
 * @returns The imported module's namespace object.
 */
export async function importScript<T extends ScriptModule = ScriptModule>(
    spec: string,
): Promise<T> {
    if (typeof spec !== "string" || spec.length === 0) {
        throw new TypeError("loadScript: `spec` must be a non-empty string");
    }
    const url = toImportSpecifier(spec);
    return (await import(url)) as T;
}
