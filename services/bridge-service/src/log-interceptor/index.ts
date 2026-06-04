import { AsyncLocalStorage } from "node:async_hooks";

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

export const LOG_LEVELS = [
    "log",
    "info",
    "warn",
    "error",
    "debug",
    "trace",
] as const;

export type LogLevel = (typeof LOG_LEVELS)[number];

export type ConsoleMethod = (...args: unknown[]) => void;

/**
 * Minimal subset of `console` we depend on. Keeping this narrow makes the
 * module easy to test against a plain object instead of the global console.
 */
export type ConsoleLike = Record<LogLevel, ConsoleMethod>;

export interface LogEntry {
    level: LogLevel;
    args: unknown[];
    time: number;
}

/** Returns a millisecond timestamp. Defaults to `Date.now`. */
export type Clock = (() => number) | { now: () => number };

const defaultConsole = (): ConsoleLike => console as unknown as ConsoleLike;

const defaultClock: Clock = Date;

// ---------------------------------------------------------------------------
// initMemoryLogger
// ---------------------------------------------------------------------------

export interface MemoryLoggerOptions {
    /** Forward calls to the original console methods. Defaults to `true`. */
    passthrough?: boolean;
    /** Cap on buffered entries; oldest entries are dropped. Defaults to `Infinity`. */
    max?: number;
    /** Console-like object to patch. Defaults to the global `console`. */
    target?: ConsoleLike;
    /** Timestamp source for entries. Defaults to `Date.now`. */
    clock?: Clock;
}

export interface MemoryLogger {
    logs: LogEntry[];
    snapshot: () => LogEntry[];
    clear: () => void;
    restore: () => void;
}

export function initLogInterceptor({
    passthrough = true,
    max = Number.POSITIVE_INFINITY,
    target = defaultConsole(),
    clock = defaultClock,
}: MemoryLoggerOptions = {}): MemoryLogger {
    const buffer: LogEntry[] = [];
    const original = {} as Record<LogLevel, ConsoleMethod>;

    for (const level of LOG_LEVELS) {
        original[level] = target[level];
        target[level] = (...args: unknown[]) => {
            buffer.push({
                level,
                args,
                time: now(clock),
            });
            if (buffer.length > max) buffer.shift();
            if (passthrough) original[level](...args);
        };
    }

    return {
        logs: buffer,
        snapshot: () => buffer.slice(),
        clear: () => {
            buffer.length = 0;
        },
        restore: () => {
            for (const level of LOG_LEVELS) target[level] = original[level];
        },
    };
}

// ---------------------------------------------------------------------------
// Scoped (concurrent-safe) log interception
// ---------------------------------------------------------------------------

interface Scope {
    buffer: LogEntry[];
    max: number;
    passthrough: boolean;
}

export interface InterceptOptions {
    /**
     * If `true` (default) logs still reach the original console.
     * If `false`, logs are captured silently for this scope. When scopes are
     * nested, as soon as any active scope wants to swallow, the call does not
     * pass through.
     */
    passthrough?: boolean;
    /** Cap the buffer size; older entries are dropped. Defaults to `Infinity`. */
    max?: number;
}

export interface InterceptResult<T> {
    result: T;
    logs: LogEntry[];
}

export interface LogInterceptor {
    /**
     * Run `fn` and capture every `console.*` call from within its async chain
     * into a private buffer. Concurrent invocations each get their own buffer
     * via `AsyncLocalStorage`. Nested scopes also capture inner logs.
     */
    interceptLogs<T>(
        fn: () => T | Promise<T>,
        options?: InterceptOptions,
    ): Promise<InterceptResult<T>>;
    /** Patch the target console (idempotent). */
    install(): void;
    /** Restore the target console to its pre-install methods. */
    uninstall(): void;
    /** Whether the interceptor is currently patched into its target. */
    isInstalled(): boolean;
}

export interface CreateLogInterceptorOptions {
    /** Console-like object to patch. Defaults to the global `console`. */
    target?: ConsoleLike;
    /** Timestamp source for entries. Defaults to `Date.now`. */
    clock?: Clock;
}

/**
 * Creates an isolated log interceptor instance. Each instance has its own
 * `AsyncLocalStorage`, install state, and original-method snapshot, which
 * makes it trivial to test against a fake console without touching globals.
 */
export function createLogInterceptor(
    options: CreateLogInterceptorOptions = {},
): LogInterceptor {
    const target = options.target ?? defaultConsole();
    const clock = options.clock ?? defaultClock;
    const storage = new AsyncLocalStorage<Scope[]>();
    const original = {} as Record<LogLevel, ConsoleMethod>;
    let installed = false;

    function install(): void {
        if (installed) return;
        installed = true;

        for (const level of LOG_LEVELS) {
            original[level] = target[level];
            target[level] = (...args: unknown[]) => {
                const stack = storage.getStore();
                let swallow = false;

                if (stack && stack.length > 0) {
                    const entry: LogEntry = {
                        level,
                        args,
                        time: now(clock),
                    };
                    for (const scope of stack) {
                        scope.buffer.push(entry);
                        if (scope.buffer.length > scope.max) {
                            scope.buffer.shift();
                        }
                        if (!scope.passthrough) {
                            swallow = true;
                        }
                    }
                }

                if (!swallow) {
                    original[level](...args);
                }
            };
        }
    }

    function uninstall(): void {
        if (!installed) return;
        for (const level of LOG_LEVELS) {
            target[level] = original[level];
        }
        installed = false;
    }

    async function interceptLogs<T>(
        fn: () => T | Promise<T>,
        opts: InterceptOptions = {},
    ): Promise<InterceptResult<T>> {
        install();

        const scope: Scope = {
            buffer: [],
            max: opts.max ?? Number.POSITIVE_INFINITY,
            passthrough: opts.passthrough ?? true,
        };

        const parent = storage.getStore() ?? [];
        const nextStack: Scope[] = [...parent, scope];

        const result = await storage.run(nextStack, async () => fn());
        return { result, logs: scope.buffer };
    }

    return {
        interceptLogs,
        install,
        uninstall,
        isInstalled: () => installed,
    };
}
function now(clock: Clock) {
    return typeof clock === "function" ? clock() : clock.now();
}

// ---------------------------------------------------------------------------
// Default singleton bound to the real console
// ---------------------------------------------------------------------------

const defaultInterceptor = createLogInterceptor();

export function interceptLogs<T>(
    fn: () => T | Promise<T>,
    options?: InterceptOptions,
): Promise<InterceptResult<T>> {
    return defaultInterceptor.interceptLogs(fn, options);
}

export function uninstallLogInterceptor(): void {
    defaultInterceptor.uninstall();
}
