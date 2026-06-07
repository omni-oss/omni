import { AsyncLocalStorage } from "node:async_hooks";
import { LOG_LEVELS, type LogLevel } from "@omni-oss/log";

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

export type ConsoleMethod = (...args: unknown[]) => void;

/**
 * Minimal subset of `console` we depend on. Keeping this narrow makes the
 * module easy to test against a plain object instead of the global console.
 *
 * Note: `log` is intentionally not part of the supported levels. Use
 * {@link adaptConsole} to bridge a real `console` (which has `log`) onto
 * this shape.
 */
export type ConsoleLike = Record<LogLevel, ConsoleMethod>;

/**
 * The shape required by {@link adaptConsole}. A real `Console` satisfies it
 * (it has both `log` and the five log levels we care about).
 */
export interface ConsoleSource extends ConsoleLike {
    log: ConsoleMethod;
}

export interface LogEntry {
    level: LogLevel;
    args: unknown[];
    time: number;
}

/**
 * Callback invoked for every captured log entry. Listeners are called
 * synchronously, in registration order, before passthrough to the original
 * console method. Throwing listeners are caught and reported via the
 * original `console.error` so a misbehaving listener can't break logging.
 */
export type LogListener = (entry: LogEntry) => void;

/** Returns a millisecond timestamp. Defaults to `Date.now`. */
export type Clock = (() => number) | { now: () => number };

/**
 * Adapts a `console`-like source to the interceptor's {@link LogLevel} set
 * (`error`, `warn`, `info`, `debug`, `trace`).
 *
 * The returned adapter is a thin facade with getters/setters that proxy to
 * the underlying source. The only special case is `info`: writing to
 * `adapter.info` also assigns the same handler to `source.log`, so plain
 * `console.log(...)` calls flow through the same patched function and are
 * captured as level `"info"` by the interceptor.
 *
 * Reading from the adapter always returns the source's current method, so
 * the interceptor's snapshot/restore logic still observes the real original
 * implementations.
 */
export function adaptConsole(
    source: ConsoleSource = console as unknown as ConsoleSource,
): ConsoleLike {
    return {
        get error() {
            return source.error;
        },
        set error(fn: ConsoleMethod) {
            source.error = fn;
        },
        get warn() {
            return source.warn;
        },
        set warn(fn: ConsoleMethod) {
            source.warn = fn;
        },
        get info() {
            return source.info;
        },
        set info(fn: ConsoleMethod) {
            source.info = fn;
            // Route `console.log(...)` calls through the same handler so they
            // get captured as level "info".
            source.log = fn;
        },
        get debug() {
            return source.debug;
        },
        set debug(fn: ConsoleMethod) {
            source.debug = fn;
        },
        get trace() {
            return source.trace;
        },
        set trace(fn: ConsoleMethod) {
            source.trace = fn;
        },
    };
}

const defaultConsole = (): ConsoleLike => adaptConsole();

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
    /**
     * Callbacks invoked with each captured `LogEntry` as it arrives, in
     * registration order, before passthrough. The interceptor keeps a
     * reference to this array, so push/splice operations on it are reflected
     * on subsequent log calls.
     */
    listeners?: LogListener[];
}

export interface MemoryLogger {
    logs: LogEntry[];
    snapshot: () => LogEntry[];
    clear: () => void;
    restore: () => void;
    /** Append a listener invoked for each subsequent log entry. */
    addListener: (listener: LogListener) => void;
    /** Remove a previously added listener. Returns `true` if removed. */
    removeListener: (listener: LogListener) => boolean;
}

export function initLogInterceptor({
    passthrough = true,
    max = Number.POSITIVE_INFINITY,
    target = defaultConsole(),
    clock = defaultClock,
    listeners = [],
}: MemoryLoggerOptions = {}): MemoryLogger {
    const buffer: LogEntry[] = [];
    const original = {} as Record<LogLevel, ConsoleMethod>;

    for (const level of LOG_LEVELS) {
        original[level] = target[level];
        target[level] = (...args: unknown[]) => {
            const entry: LogEntry = {
                level,
                args,
                time: now(clock),
            };
            buffer.push(entry);
            if (buffer.length > max) buffer.shift();
            notifyListeners(listeners, entry, original.error);
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
        addListener: (listener) => {
            listeners.push(listener);
        },
        removeListener: (listener) => {
            const idx = listeners.indexOf(listener);
            if (idx === -1) return false;
            listeners.splice(idx, 1);
            return true;
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
    listeners: LogListener[];
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
    /**
     * Callbacks invoked with each captured `LogEntry` for this scope, in
     * registration order. The array reference is retained, so mutations are
     * picked up on subsequent log calls within the scope.
     */
    listeners?: LogListener[];
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
    /**
     * Register a global listener that receives every captured `LogEntry`,
     * regardless of whether a scope is active. Returns a disposer.
     */
    addListener(listener: LogListener): () => void;
    /** Remove a previously added global listener. Returns `true` if removed. */
    removeListener(listener: LogListener): boolean;
}

export interface CreateLogInterceptorOptions {
    /** Console-like object to patch. Defaults to the global `console`. */
    target?: ConsoleLike;
    /** Timestamp source for entries. Defaults to `Date.now`. */
    clock?: Clock;
    /**
     * Global listeners invoked for every captured entry across all scopes
     * (and even when no scope is active). The array reference is retained,
     * so mutations are picked up by subsequent log calls.
     */
    listeners?: LogListener[];
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
    const globalListeners: LogListener[] = options.listeners ?? [];
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

                const entry: LogEntry = {
                    level,
                    args,
                    time: now(clock),
                };

                if (stack && stack.length > 0) {
                    for (const scope of stack) {
                        scope.buffer.push(entry);
                        if (scope.buffer.length > scope.max) {
                            scope.buffer.shift();
                        }
                        notifyListeners(scope.listeners, entry, original.error);
                        if (!scope.passthrough) {
                            swallow = true;
                        }
                    }
                }

                notifyListeners(globalListeners, entry, original.error);

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
            listeners: opts.listeners ?? [],
        };

        const parent = storage.getStore() ?? [];
        const nextStack: Scope[] = [...parent, scope];

        const result = await storage.run(nextStack, async () => fn());
        return { result, logs: scope.buffer };
    }

    function addListener(listener: LogListener): () => void {
        globalListeners.push(listener);
        return () => {
            removeListener(listener);
        };
    }

    function removeListener(listener: LogListener): boolean {
        const idx = globalListeners.indexOf(listener);
        if (idx === -1) return false;
        globalListeners.splice(idx, 1);
        return true;
    }

    return {
        interceptLogs,
        install,
        uninstall,
        isInstalled: () => installed,
        addListener,
        removeListener,
    };
}
function now(clock: Clock) {
    return typeof clock === "function" ? clock() : clock.now();
}

/**
 * Invoke each listener with the entry, isolating failures so a buggy listener
 * cannot poison logging for the rest of the chain. We iterate over a snapshot
 * so listeners that mutate the array during dispatch don't skip or revisit
 * siblings.
 */
function notifyListeners(
    listeners: readonly LogListener[],
    entry: LogEntry,
    reportError: ConsoleMethod | undefined,
): void {
    if (listeners.length === 0) return;
    const snapshot = listeners.slice();
    for (const listener of snapshot) {
        try {
            listener(entry);
        } catch (err) {
            reportError?.("log listener threw:", err);
        }
    }
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

/**
 * Register a global listener on the default singleton interceptor. Returns a
 * disposer that removes the listener when called.
 */
export function addLogListener(listener: LogListener): () => void {
    return defaultInterceptor.addListener(listener);
}

/**
 * Remove a previously added global listener from the default singleton
 * interceptor. Returns `true` if it was found and removed.
 */
export function removeLogListener(listener: LogListener): boolean {
    return defaultInterceptor.removeListener(listener);
}
