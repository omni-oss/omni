import { isAbsolute, resolve as resolvePath } from "node:path";
import { pathToFileURL } from "node:url";

// ──────────────────────────────────────────────────────────────────────────
// Specifier normalization
//
// Accepted script specs:
//   ./foo.ts | ../foo.ts          → relative path
//   /abs/foo.ts                   → POSIX absolute path
//   C:\scripts\foo.ts             → Windows absolute path
//   \\?\C:\scripts\foo.ts         → Windows extended-length path (from Rust canonicalize)
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

/**
 * Strip the Windows extended-length path prefix (`\\?\` or `\\?\UNC\`) that
 * Rust's `std::fs::canonicalize` produces on Windows. `pathToFileURL` cannot
 * handle it and generates an invalid `file://%3F\…` URL instead.
 *
 * - `\\?\UNC\server\share` → `\\server\share`
 * - `\\?\C:\…`             → `C:\…`
 */
function stripWindowsExtendedLengthPrefix(p: string): string {
    // \\?\UNC\server\share  →  \\server\share
    if (p.startsWith("\\\\?\\UNC\\") || p.startsWith("\\\\?\\UNC/")) {
        return `\\\\${p.slice(8)}`;
    }
    // \\?\C:\…  →  C:\…
    if (p.startsWith("\\\\?\\")) {
        return p.slice(4);
    }
    return p;
}

function toImportSpecifier(spec: string): string {
    if (isUrlLike(spec) || isBare(spec)) return spec;
    // Strip Windows extended-length path prefix before converting to a file
    // URL.  Without this, pathToFileURL produces an invalid `file://%3F\…`
    // URL for paths returned by Rust's std::fs::canonicalize on Windows.
    const normalized = stripWindowsExtendedLengthPrefix(spec);
    // Relative or absolute filesystem path → file URL (Windows-safe).
    return pathToFileURL(resolvePath(normalized)).href;
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
