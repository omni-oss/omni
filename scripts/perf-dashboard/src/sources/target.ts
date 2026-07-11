import type { TargetId } from "./types";

/**
 * Shared os/arch → target mapping. Used by the local disk source to infer a
 * target from a payload's `platform.os`, and (later) by the GitHub source to
 * validate target directories. See DESIGN.md §4.
 */

/** The published target directories, in canonical order. */
export const KNOWN_TARGETS: readonly TargetId[] = [
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "aarch64-apple-darwin",
];

const KNOWN_TARGET_SET = new Set<string>(KNOWN_TARGETS);

export function isKnownTarget(value: string): value is TargetId {
    return KNOWN_TARGET_SET.has(value);
}

/**
 * Map a Node-style `os.platform()` + `os.arch()` pair to a canonical target.
 * Returns null for combinations we don't publish.
 */
export function osArchToTarget(
    platform: string,
    arch: string,
): TargetId | null {
    const p = platform.toLowerCase();
    const a = arch.toLowerCase();
    if (p === "linux" && (a === "x64" || a === "x86_64")) {
        return "x86_64-unknown-linux-gnu";
    }
    if (p === "win32" && (a === "x64" || a === "x86_64")) {
        return "x86_64-pc-windows-msvc";
    }
    if (p === "darwin" && (a === "arm64" || a === "aarch64")) {
        return "aarch64-apple-darwin";
    }
    return null;
}

/** Best-effort OS family ("linux" | "win32" | "darwin") for a target. */
export function targetToOs(target: TargetId): string | null {
    if (target.includes("linux")) return "linux";
    if (target.includes("windows")) return "win32";
    if (target.includes("apple") || target.includes("darwin")) return "darwin";
    return null;
}
