import { platform } from "node:os";

/**
 * Exit codes that signal an *unrecoverable* host/environment failure rather
 * than a flaky run. Retrying these is pointless and often actively harmful:
 * on Windows the common one is a process-initialization failure caused by
 * session/desktop-heap exhaustion, so spawning more processes only deepens the
 * hole (see RFC discussion / the CI cascade this guards against).
 *
 * Keyed by `process.platform`, then by the raw (unsigned) exit code, with the
 * value explaining *why* the code isn't worth retrying so it can be surfaced
 * to the user verbatim.
 */
const UNRECOVERABLE_EXIT_CODES: Record<string, Record<number, string>> = {
    win32: {
        // 0xC0000142 STATUS_DLL_INIT_FAILED
        3221225794:
            "Windows could not initialize the process (STATUS_DLL_INIT_FAILED, 0xC0000142) — almost always session/desktop-heap exhaustion from spawning too many processes; retrying spawns more and makes it worse.",
        // 0xC0000017 STATUS_NO_MEMORY
        3221225495:
            "Windows is out of memory (STATUS_NO_MEMORY, 0xC0000017) — the host cannot allocate for a new process; a retry cannot free memory.",
        // 0xC0000135 STATUS_DLL_NOT_FOUND
        3221225781:
            "A required DLL was not found (STATUS_DLL_NOT_FOUND, 0xC0000135) — the environment is misconfigured, not transient.",
    },
    linux: {
        // 128 + SIGKILL(9): almost always the OOM killer under memory pressure.
        137: "Process was killed with SIGKILL (exit 137) — typically the Linux OOM killer; retrying under the same memory pressure will fail again.",
    },
    darwin: {
        // 128 + SIGKILL(9): typically memory pressure / jetsam.
        137: "Process was killed with SIGKILL (exit 137) — typically memory pressure; retrying under the same conditions will fail again.",
    },
};

/**
 * The reason `exitCode` is unrecoverable on the given platform (defaults to the
 * current one), or `null` when the failure is safe to retry.
 */
export function unrecoverableExitReason(
    exitCode: number,
    os: string = platform(),
): string | null {
    return UNRECOVERABLE_EXIT_CODES[os]?.[exitCode] ?? null;
}
