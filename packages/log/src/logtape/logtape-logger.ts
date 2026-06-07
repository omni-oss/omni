import {
    getLogger,
    type LogLevel as LogTapeLevel,
    type Logger as LogTapeLoggerImpl,
} from "@logtape/logtape";
import {
    AbstractLogger,
    type CategoryParam,
    type Logger,
    type LoggerFactory,
    type LogLevel,
} from "@/core";

export class LogTapeLogger extends AbstractLogger {
    /**
     * Cache key shared with the {@link LogTapeLoggerFactory} that produced
     * this wrapper (or any ancestor it was derived from via {@link child}).
     *
     * Every wrapper in a single factory's family carries the same Symbol,
     * which is also used for the factory's own root-level cache. This way:
     *
     * - `factory.get(category)` and `parent.child(subcategory)` both look
     *   up the same slot on the underlying logtape Logger, so
     *   `factory.get(["a", "b"])` and `factory.get(["a"]).child("b")`
     *   produce the same wrapper (whichever path runs first installs it).
     * - The {@link parent} getter resolves the parent wrapper through the
     *   same cache, so the parent chain is reference-stable end-to-end.
     * - Two different factories use different Symbols, so their families
     *   stay isolated.
     * - A wrapper constructed directly (without a factory) gets its own
     *   default Symbol, which is independent of any factory.
     *
     * In other words: every wrapper traces back to the factory whose
     * Symbol it carries.
     */
    private readonly cacheKey: symbol;

    constructor(
        private ltLogger: LogTapeLoggerImpl,
        cacheKey: symbol = Symbol("LogTapeLoggerFactory.cache"),
    ) {
        super();
        this.cacheKey = cacheKey;
        // Self-attach: claim our slot on the underlying logtape Logger so
        // that subsequent lookups from sibling code paths (the {@link parent}
        // getter, {@link child} on a parallel wrapper, factory.get) resolve
        // back to *this* instance. First-write-wins: if the slot is already
        // taken under this cacheKey, the existing wrapper is preserved and
        // this one becomes "orphaned" w.r.t. the cache.
        const slot = ltLogger as unknown as {
            [k: symbol]: Logger | undefined;
        };
        if (slot[cacheKey] === undefined) {
            slot[cacheKey] = this;
        }
    }

    /**
     * Resolves the parent wrapper dynamically from the underlying logtape
     * Logger's `parent` chain. The result is wrapped using this wrapper's
     * own cacheKey, so the entire parent chain is reference-stable within
     * a single factory's family.
     *
     * Returns `null` when the underlying logtape Logger has no parent
     * (i.e. it is logtape's global root).
     */
    override get parent(): Logger | null {
        const ltParent = this.ltLogger.parent;
        if (ltParent == null) return null;
        return getOrAttachWrapper(
            ltParent,
            this.cacheKey,
            () => new LogTapeLogger(ltParent, this.cacheKey),
        );
    }

    override enabled(level: LogLevel): boolean {
        return this.ltLogger.isEnabledFor(conv(level));
    }

    override child(subcategory: CategoryParam): Logger {
        const childLogTapeLogger = this.ltLogger.getChild(subcategory);
        return getOrAttachWrapper(
            childLogTapeLogger,
            this.cacheKey,
            () => new LogTapeLogger(childLogTapeLogger, this.cacheKey),
        );
    }

    override with(properties: Record<string, unknown>): Logger {
        const childLogTapeLogger = this.ltLogger.with(properties);
        // `with()` is intentionally not cached: logtape's `LoggerCtx`
        // returns a fresh contextual wrapper for each call, so a cache
        // lookup would never hit. The new wrapper still self-attaches
        // under its own cacheKey via the constructor.
        return new LogTapeLogger(childLogTapeLogger, this.cacheKey);
    }

    logEager = (
        level: LogLevel,
        message: string,
        properties?: Record<string, unknown>,
    ): void => {
        this.logFn(level)(message, properties);
    };

    logLazy = (
        level: LogLevel,
        message: string,
        computeProperties?: () => Record<string, unknown>,
    ): void => {
        this.logFn(level)(message, computeProperties);
    };

    logLazyAsync = (
        level: LogLevel,
        message: string,
        computeProperties: () => Promise<Record<string, unknown>>,
    ): Promise<void> => {
        return this.logFn(level)(message, computeProperties);
    };

    logTemplate = (
        level: LogLevel,
        template: TemplateStringsArray,
        args: unknown[],
    ): void => {
        // logtape's tagged-template signature is `(template, ...values)`; we
        // must spread `args` so each interpolation lands in its own slot of
        // the resulting `LogRecord.message` array. Passing `args` as a single
        // argument would nest the array inside `message`.
        this.logFn(level)(template, ...args);
    };

    private logFn(level: LogLevel) {
        const log = this.ltLogger[conv(level)];

        if (!log) {
            throw new Error(`Unsupported log level: ${level}`);
        }

        // Bind to ltLogger so that internal `this` references inside
        // logtape's method bodies (e.g. `this.log`, `this.isEnabledFor`)
        // resolve correctly. Pulling the reference off the object loses
        // the implicit binding, which crashes inside logtape v2.
        return log.bind(this.ltLogger);
    }
}

export class LogTapeLoggerFactory implements LoggerFactory {
    /**
     * Per-factory cache key. The same Symbol is threaded into every
     * {@link LogTapeLogger} the factory creates (and into all of their
     * `.child()` descendants), so any wrapper in the family can be traced
     * back to this factory by inspecting its cache key.
     *
     * See {@link LogTapeLogger.cacheKey} for the broader rationale.
     */
    private readonly cacheKey: symbol = Symbol("LogTapeLoggerFactory.cache");

    get(category: CategoryParam): Logger {
        const ltLogger = getLogger(category);
        return getOrAttachWrapper(
            ltLogger,
            this.cacheKey,
            () => new LogTapeLogger(ltLogger, this.cacheKey),
        );
    }
}

/**
 * O(1) cache helper shared by {@link LogTapeLogger.child} and
 * {@link LogTapeLoggerFactory.get}. Looks up a {@link Logger} wrapper
 * stored under `cacheKey` on `target`; if absent, builds one via `create`,
 * stashes it, and returns it.
 *
 * `target` is treated as an opaque object onto which we attach a
 * Symbol-keyed property. Logtape's own `Logger` instances are reference-
 * stable per category (its `getLogger` and `getChild` are internally
 * memoised), so attaching the wrapper directly gives us a property read
 * for every cache hit — no separate `Map` required.
 *
 * The `cacheKey` Symbol is intended to be unique per *cache owner*
 * (a factory instance, or a `LogTapeLogger` instance for its child
 * cache), so independent owners can co-exist on the same underlying
 * logtape Logger without colliding.
 */
function getOrAttachWrapper(
    target: LogTapeLoggerImpl,
    cacheKey: symbol,
    create: () => Logger,
): Logger {
    const slot = target as unknown as { [k: symbol]: Logger | undefined };
    const cached = slot[cacheKey];
    if (cached !== undefined) {
        return cached;
    }
    const wrapper = create();
    slot[cacheKey] = wrapper;
    return wrapper;
}

function conv(level: LogLevel): LogTapeLevel {
    switch (level) {
        case "warn":
            return "warning";
        default:
            return level;
    }
}
