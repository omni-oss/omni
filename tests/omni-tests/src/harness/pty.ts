/**
 * Pseudo-terminal harness for interactive / TUI e2e tests.
 *
 * Some omni flows are interactive: `+generator` inputs, `+prompt` confirmations,
 * spinners, and other TUI surfaces only render (and only read input) when stdin
 * is a real TTY. {@link runOmni} pipes through plain pipes, so it can't exercise
 * those paths. This harness instead spawns the binary under a pty (via
 * `node-pty`) and feeds the raw output into a headless xterm terminal
 * (`@xterm/headless`) so tests can assert on the *rendered* screen rather than
 * the raw byte stream (which is full of cursor moves and redraws).
 *
 * Typical use:
 *
 * ```ts
 * const ws = makeWorkspace(singleProjectSpec());
 * const pty = spawnOmniPty(["generator", "run", "thing"], { cwd: ws.cwd });
 * await pty.waitFor(/Project name/);
 * pty.type("my-app");
 * pty.press("enter");
 * const result = await pty.waitForExit();
 * expect(result.exitCode).toBe(0);
 * ```
 *
 * Like {@link makeWorkspace}, sessions auto-dispose after the current test when
 * created inside one.
 */

import { Terminal } from "@xterm/headless";
import * as pty from "node-pty";
import { onTestFinished } from "vitest";
import { resolveOmniBin } from "./binary";
import { normalize } from "./normalize";

/** Named keys recognized by {@link PtySession.press}, mapped to control bytes. */
const KEYS = {
    enter: "\r",
    tab: "\t",
    escape: "\x1b",
    space: " ",
    backspace: "\x7f",
    delete: "\x1b[3~",
    up: "\x1b[A",
    down: "\x1b[B",
    right: "\x1b[C",
    left: "\x1b[D",
    home: "\x1b[H",
    end: "\x1b[F",
    pageUp: "\x1b[5~",
    pageDown: "\x1b[6~",
    ctrlC: "\x03",
    ctrlD: "\x04",
} as const;

export type PtyKey = keyof typeof KEYS;

/** Anything that can be waited for: substring, regexp, or screen predicate. */
export type ScreenMatcher = string | RegExp | ((screen: string) => boolean);

export interface SpawnOmniPtyOptions {
    /** Working directory to run in (usually a workspace root). */
    cwd?: string;
    /**
     * Environment variables to set. Merged on top of the parent process env
     * (set {@link SpawnOmniPtyOptions.cleanEnv} to start from empty instead).
     * `undefined` values are dropped, since pty envs must be strings.
     */
    env?: Record<string, string | undefined>;
    /** When true, do not inherit the parent process environment. */
    cleanEnv?: boolean;
    /** Terminal width in columns. Default 80. */
    cols?: number;
    /** Terminal height in rows. Default 30. */
    rows?: number;
    /**
     * `TERM` value advertised to the child. Default `xterm-256color` so omni
     * renders colored/interactive output as it would in a real terminal.
     */
    term?: string;
}

export interface WaitForOptions {
    /** Milliseconds to wait before rejecting. Default 10_000. */
    timeout?: number;
}

export interface PtyExit {
    /** Process exit code (0 on success). */
    exitCode: number;
    /** Terminating signal number, or 0 when none. */
    signal: number;
}

export interface PtySession {
    /** Write a raw string to the child's stdin (no implicit newline). */
    write(data: string): void;
    /** Alias of {@link PtySession.write}, reads better for user input. */
    type(data: string): void;
    /** Write a named key's control sequence (e.g. `press("enter")`). */
    press(key: PtyKey): void;
    /**
     * The rendered visible viewport as plain text, trailing blank lines and
     * per-line trailing whitespace stripped. This is what a user would see.
     */
    screen(): string;
    /**
     * The full terminal contents including scrollback (clean text). Use this
     * when output you care about may have scrolled out of the viewport.
     */
    text(): string;
    /** Raw bytes received so far, including escape sequences (for debugging). */
    raw(): string;
    /**
     * Resolve once the rendered terminal text matches. Strings/regexps are
     * tested against {@link PtySession.text} (scrollback included); a predicate
     * receives the same text. Rejects on timeout with the current screen.
     */
    waitFor(matcher: ScreenMatcher, options?: WaitForOptions): Promise<void>;
    /** Resolve when the child exits (immediately if it already has). */
    waitForExit(options?: WaitForOptions): Promise<PtyExit>;
    /** Resize the pty + terminal (for testing responsive/redraw behavior). */
    resize(cols: number, rows: number): void;
    /** Kill the child and dispose the terminal. Safe to call multiple times. */
    dispose(): void;
    /** True once the child process has exited. */
    readonly exited: boolean;
    /** Exit info once the child has exited, otherwise `undefined`. */
    readonly exit: PtyExit | undefined;
    /** The full command line that was spawned (for diagnostics). */
    readonly command: string;
}

const DEFAULT_COLS = 80;
const DEFAULT_ROWS = 30;
const DEFAULT_WAIT_TIMEOUT_MS = 10_000;

interface Waiter {
    predicate: (text: string) => boolean;
    resolve: () => void;
    reject: (error: Error) => void;
    timer: ReturnType<typeof setTimeout>;
    describe: string;
}

function toPredicate(matcher: ScreenMatcher): (text: string) => boolean {
    if (typeof matcher === "function") return matcher;
    if (matcher instanceof RegExp) {
        // Clone without the global flag so repeated tests don't advance lastIndex.
        const pattern = new RegExp(
            matcher.source,
            matcher.flags.replace("g", ""),
        );
        return (text) => pattern.test(text);
    }
    return (text) => text.includes(matcher);
}

function describeMatcher(matcher: ScreenMatcher): string {
    if (typeof matcher === "function") return matcher.name || "<predicate>";
    if (matcher instanceof RegExp) return matcher.toString();
    return JSON.stringify(matcher);
}

function buildEnv(options: SpawnOmniPtyOptions): Record<string, string> {
    const base = options.cleanEnv ? {} : process.env;
    const merged: Record<string, string> = {};
    for (const [key, value] of Object.entries(base)) {
        if (typeof value === "string") merged[key] = value;
    }
    for (const [key, value] of Object.entries(options.env ?? {})) {
        if (value !== undefined) merged[key] = value;
    }
    merged.TERM = options.term ?? "xterm-256color";
    return merged;
}

function tryRegisterCleanup(cleanup: () => void): void {
    try {
        // Throws when there is no active test; harmless when used outside one.
        onTestFinished(() => cleanup());
    } catch {
        // Caller owns dispose().
    }
}

/**
 * Spawn `omni <...args>` under a pseudo-terminal and return a controllable,
 * screen-aware {@link PtySession}.
 *
 * Resolves the binary the same way {@link runOmni} does (honoring
 * `OMNI_TEST_BIN` etc.). Throws only if the binary can't be spawned.
 */
export function spawnOmniPty(
    args: string[],
    options: SpawnOmniPtyOptions = {},
): PtySession {
    const bin = resolveOmniBin();
    const cols = options.cols ?? DEFAULT_COLS;
    const rows = options.rows ?? DEFAULT_ROWS;

    const term = new Terminal({ cols, rows, allowProposedApi: true });

    const child = pty.spawn(bin, args, {
        name: options.term ?? "xterm-256color",
        cols,
        rows,
        cwd: options.cwd ?? process.cwd(),
        env: buildEnv(options),
    });

    const waiters = new Set<Waiter>();
    let raw = "";
    let exit: PtyExit | undefined;
    let disposed = false;
    const exitWaiters = new Set<{
        resolve: (exit: PtyExit) => void;
        timer: ReturnType<typeof setTimeout> | undefined;
    }>();

    function viewport(): string {
        const buffer = term.buffer.active;
        const lines: string[] = [];
        for (let y = 0; y < term.rows; y++) {
            const line = buffer.getLine(buffer.baseY + y);
            lines.push(line ? line.translateToString(true) : "");
        }
        while (lines.length > 0 && lines[lines.length - 1] === "") {
            lines.pop();
        }
        return lines.join("\n");
    }

    function fullText(): string {
        const buffer = term.buffer.active;
        const total = buffer.baseY + term.rows;
        const lines: string[] = [];
        for (let y = 0; y < total; y++) {
            const line = buffer.getLine(y);
            lines.push(line ? line.translateToString(true) : "");
        }
        while (lines.length > 0 && lines[lines.length - 1] === "") {
            lines.pop();
        }
        return lines.join("\n");
    }

    function flushWaiters(): void {
        if (waiters.size === 0) return;
        const text = fullText();
        for (const waiter of waiters) {
            if (waiter.predicate(text)) {
                clearTimeout(waiter.timer);
                waiters.delete(waiter);
                waiter.resolve();
            }
        }
    }

    child.onData((data) => {
        raw += data;
        // term.write is async; only re-check waiters once the chunk is parsed.
        term.write(data, flushWaiters);
    });

    // Interactive TUIs (requestty/crossterm) probe the terminal with queries
    // like cursor-position / device-status reports (`ESC[6n`) and block on the
    // reply. A real terminal answers automatically; the headless xterm emits
    // those answers via onData, so forward them back to the child - otherwise
    // the program hangs waiting for a response that never comes.
    term.onData((reply) => {
        if (!disposed) {
            child.write(reply);
        }
    });

    child.onExit(({ exitCode, signal }) => {
        exit = { exitCode, signal: signal ?? 0 };
        for (const waiter of exitWaiters) {
            if (waiter.timer) clearTimeout(waiter.timer);
            waiter.resolve(exit);
        }
        exitWaiters.clear();
        // Give any buffered output a chance to satisfy pending screen waiters.
        flushWaiters();
    });

    const command = [bin, ...args].join(" ");

    const session: PtySession = {
        write(data) {
            child.write(data);
        },
        type(data) {
            child.write(data);
        },
        press(key) {
            child.write(KEYS[key]);
        },
        screen: () => normalize(viewport()),
        text: () => normalize(fullText()),
        raw: () => raw,
        waitFor(matcher, waitOptions = {}) {
            const predicate = toPredicate(matcher);
            if (predicate(fullText())) return Promise.resolve();
            const timeout = waitOptions.timeout ?? DEFAULT_WAIT_TIMEOUT_MS;
            const describe = describeMatcher(matcher);
            return new Promise<void>((resolve, reject) => {
                const waiter: Waiter = {
                    predicate,
                    resolve,
                    reject,
                    describe,
                    timer: setTimeout(() => {
                        waiters.delete(waiter);
                        reject(
                            new Error(
                                `Timed out after ${timeout}ms waiting for ${describe}.\n` +
                                    `Command: ${command}\nScreen:\n${normalize(viewport())}`,
                            ),
                        );
                    }, timeout),
                };
                waiters.add(waiter);
            });
        },
        waitForExit(waitOptions = {}) {
            if (exit) return Promise.resolve(exit);
            const timeout = waitOptions.timeout ?? DEFAULT_WAIT_TIMEOUT_MS;
            return new Promise<PtyExit>((resolve, reject) => {
                const entry = {
                    resolve,
                    timer: setTimeout(() => {
                        exitWaiters.delete(entry);
                        reject(
                            new Error(
                                `Timed out after ${timeout}ms waiting for exit.\n` +
                                    `Command: ${command}\nScreen:\n${normalize(viewport())}`,
                            ),
                        );
                    }, timeout),
                };
                exitWaiters.add(entry);
            });
        },
        resize(nextCols, nextRows) {
            child.resize(nextCols, nextRows);
            term.resize(nextCols, nextRows);
        },
        dispose() {
            if (disposed) return;
            disposed = true;
            for (const waiter of waiters) {
                clearTimeout(waiter.timer);
                waiter.reject(new Error("PTY session disposed before match."));
            }
            waiters.clear();
            for (const entry of exitWaiters) {
                if (entry.timer) clearTimeout(entry.timer);
            }
            exitWaiters.clear();
            try {
                child.kill();
            } catch {
                // Already exited.
            }
            term.dispose();
        },
        get exited() {
            return exit !== undefined;
        },
        get exit() {
            return exit;
        },
        command,
    };

    tryRegisterCleanup(() => session.dispose());

    return session;
}
