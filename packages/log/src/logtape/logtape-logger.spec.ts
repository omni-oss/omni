import type { Logger as LogTapeLoggerImpl } from "@logtape/logtape";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LOG_LEVELS, type LogLevel } from "../core";
import { LogTapeLogger, LogTapeLoggerFactory } from "./logtape-logger";

// Mock the underlying logtape module so we can intercept `getLogger`. The
// factory test uses this; LogTapeLogger tests construct the wrapper directly
// with a hand-rolled mock and never reach this module.
vi.mock("@logtape/logtape", () => ({
    getLogger: vi.fn(),
}));

import { getLogger } from "@logtape/logtape";

/**
 * Shape of the logtape Logger surface that LogTapeLogger actually uses.
 * Anything we don't touch is omitted; the cast in `createMockLtLogger`
 * widens the value to LogTapeLoggerImpl for the constructor.
 */
interface MockLtLogger {
    parent: LogTapeLoggerImpl | null;
    trace: ReturnType<typeof vi.fn>;
    debug: ReturnType<typeof vi.fn>;
    info: ReturnType<typeof vi.fn>;
    warning: ReturnType<typeof vi.fn>;
    error: ReturnType<typeof vi.fn>;
    fatal: ReturnType<typeof vi.fn>;
    getChild: ReturnType<typeof vi.fn>;
    isEnabledFor: ReturnType<typeof vi.fn>;
    with: ReturnType<typeof vi.fn>;
}

function createMockLtLogger(): MockLtLogger {
    return {
        // Default to null — most tests don't care about the parent chain.
        // Tests that exercise the `parent` getter override this explicitly.
        parent: null,
        trace: vi.fn(),
        debug: vi.fn(),
        info: vi.fn(),
        warning: vi.fn(),
        error: vi.fn(),
        fatal: vi.fn(),
        getChild: vi.fn(),
        isEnabledFor: vi.fn(),
        with: vi.fn(),
    };
}

function makeLogger(mock: MockLtLogger = createMockLtLogger()): {
    logger: LogTapeLogger;
    mock: MockLtLogger;
} {
    const logger = new LogTapeLogger(mock as unknown as LogTapeLoggerImpl);
    return { logger, mock };
}

/**
 * Maps each application-level LogLevel onto the logtape method name that
 * should ultimately receive the call. Note that "warn" maps to "warning"
 * on the logtape side (handled by the `conv` helper inside the wrapper).
 */
const LEVEL_TO_LT_METHOD: Record<
    LogLevel,
    "trace" | "debug" | "info" | "warning" | "error"
> = {
    trace: "trace",
    debug: "debug",
    info: "info",
    warn: "warning",
    error: "error",
};

describe("LogTapeLogger constructor & parent", () => {
    it("parent is null when ltLogger.parent is null", () => {
        const { logger } = makeLogger();
        expect(logger.parent).toBeNull();
    });

    it("parent getter resolves dynamically via ltLogger.parent", () => {
        // Wire up a logtape-shaped parent chain on the underlying mocks:
        //   childMock.parent -> parentMock
        // The wrapper's `parent` getter walks this chain and wraps the
        // result under the same cacheKey so the existing parent wrapper
        // is returned (not a fresh one).
        const parentMock = createMockLtLogger();
        const childMock = createMockLtLogger();
        childMock.parent = parentMock as unknown as LogTapeLoggerImpl;

        // Use the same cacheKey for both so the cache lookup unifies.
        const cacheKey = Symbol("test");
        const parent = new LogTapeLogger(
            parentMock as unknown as LogTapeLoggerImpl,
            cacheKey,
        );
        const child = new LogTapeLogger(
            childMock as unknown as LogTapeLoggerImpl,
            cacheKey,
        );

        // The constructor self-attached `parent` to parentMock under
        // cacheKey, so the child's getter resolves back to that very
        // wrapper.
        expect(child.parent).toBe(parent);
    });

    it("parent getter returns the same reference on repeated reads (cached)", () => {
        const parentMock = createMockLtLogger();
        const childMock = createMockLtLogger();
        childMock.parent = parentMock as unknown as LogTapeLoggerImpl;

        const child = new LogTapeLogger(
            childMock as unknown as LogTapeLoggerImpl,
        );

        const a = child.parent;
        const b = child.parent;
        expect(a).not.toBeNull();
        expect(a).toBe(b);
    });
});

describe("LogTapeLogger.enabled", () => {
    it("delegates to ltLogger.isEnabledFor for each level", () => {
        for (const level of LOG_LEVELS) {
            const { logger, mock } = makeLogger();
            mock.isEnabledFor.mockReturnValue(true);

            const result = logger.enabled(level);

            expect(result).toBe(true);
            expect(mock.isEnabledFor).toHaveBeenCalledTimes(1);
            expect(mock.isEnabledFor).toHaveBeenCalledWith(
                LEVEL_TO_LT_METHOD[level],
            );
        }
    });

    it("returns false when ltLogger.isEnabledFor returns false", () => {
        const { logger, mock } = makeLogger();
        mock.isEnabledFor.mockReturnValue(false);

        expect(logger.enabled("info")).toBe(false);
    });

    it("translates 'warn' to logtape 'warning'", () => {
        const { logger, mock } = makeLogger();
        mock.isEnabledFor.mockReturnValue(true);

        logger.enabled("warn");

        expect(mock.isEnabledFor).toHaveBeenCalledWith("warning");
    });
});

describe("LogTapeLogger.child", () => {
    it("delegates to ltLogger.getChild with the subcategory", () => {
        const { logger, mock } = makeLogger();
        const childLt = createMockLtLogger();
        mock.getChild.mockReturnValue(childLt);

        logger.child("sub");

        expect(mock.getChild).toHaveBeenCalledTimes(1);
        expect(mock.getChild).toHaveBeenCalledWith("sub");
    });

    it("supports tuple-style category arguments", () => {
        const { logger, mock } = makeLogger();
        mock.getChild.mockReturnValue(createMockLtLogger());

        logger.child(["a", "b"]);

        expect(mock.getChild).toHaveBeenCalledWith(["a", "b"]);
    });

    it("returns a LogTapeLogger wrapping the child ltLogger", () => {
        const { logger, mock } = makeLogger();
        const childLt = createMockLtLogger();
        mock.getChild.mockReturnValue(childLt);

        const child = logger.child("sub");

        expect(child).toBeInstanceOf(LogTapeLogger);
        expect(child).not.toBe(logger);
    });

    it("sets the child's parent to the original logger", () => {
        const { logger, mock } = makeLogger();
        const childLt = createMockLtLogger();
        // Wire up the logtape-shaped parent chain so the dynamic `parent`
        // getter resolves back to the original wrapper:
        childLt.parent = mock as unknown as LogTapeLoggerImpl;
        mock.getChild.mockReturnValue(childLt);

        const child = logger.child("sub");

        expect(child.parent).toBe(logger);
    });

    it("uses the child ltLogger's surface for subsequent calls", () => {
        const { logger, mock } = makeLogger();
        const childLt = createMockLtLogger();
        mock.getChild.mockReturnValue(childLt);

        const child = logger.child("sub");
        child.info("hello");

        // The child should route to the child mock, not the parent mock.
        expect(childLt.info).toHaveBeenCalledTimes(1);
        expect(mock.info).not.toHaveBeenCalled();
    });

    it("caches the child wrapper per subcategory: repeated calls return the same instance", () => {
        const { logger, mock } = makeLogger();
        const childLt = createMockLtLogger();
        mock.getChild.mockReturnValue(childLt);

        const a = logger.child("sub");
        const b = logger.child("sub");

        expect(a).toBe(b);
    });

    it("only constructs a single child wrapper for repeated child() calls (cache slot exists exactly once)", () => {
        const { logger, mock } = makeLogger();
        const childLt = createMockLtLogger();
        mock.getChild.mockReturnValue(childLt);

        logger.child("sub");
        logger.child("sub");
        logger.child("sub");

        // The wrapper is attached as a Symbol-keyed property on the child
        // logtape Logger object. Exactly one of those Symbols must exist
        // after multiple calls, no matter how many times we asked.
        //
        // The description is `LogTapeLoggerFactory.cache` because the same
        // cache key is shared between the factory and every wrapper it
        // produces (so any wrapper traces back to its factory).
        const childCacheSymbols = Object.getOwnPropertySymbols(childLt).filter(
            (s) => s.description === "LogTapeLoggerFactory.cache",
        );
        expect(childCacheSymbols).toHaveLength(1);
    });

    it("the cached child preserves its parent reference across lookups", () => {
        const { logger, mock } = makeLogger();
        const childLt = createMockLtLogger();
        // Wire up the logtape parent chain so the dynamic getter resolves
        // back to the original wrapper.
        childLt.parent = mock as unknown as LogTapeLoggerImpl;
        mock.getChild.mockReturnValue(childLt);

        const first = logger.child("sub");
        const second = logger.child("sub");

        expect(first.parent).toBe(logger);
        expect(second.parent).toBe(logger);
    });

    it("returns different cached wrappers for different subcategories", () => {
        const { logger, mock } = makeLogger();
        const childA = createMockLtLogger();
        const childB = createMockLtLogger();
        mock.getChild.mockImplementation(((sub: unknown) => {
            return (sub === "a" ? childA : childB) as unknown;
        }) as typeof mock.getChild);

        const a = logger.child("a");
        const b = logger.child("b");

        expect(a).not.toBe(b);
        // Each subcategory is still individually cache-stable:
        expect(logger.child("a")).toBe(a);
        expect(logger.child("b")).toBe(b);
    });

    it("two parent wrappers wrapping the same ltLogger produce distinct child wrappers (independent caches)", () => {
        // Same ltLogger, two different LogTapeLogger wrappers around it.
        const sharedLt = createMockLtLogger();
        const childLt = createMockLtLogger();
        // The dynamic parent getter walks the logtape chain, so wire it up.
        childLt.parent = sharedLt as unknown as LogTapeLoggerImpl;
        sharedLt.getChild.mockReturnValue(childLt);

        const parent1 = new LogTapeLogger(
            sharedLt as unknown as LogTapeLoggerImpl,
        );
        const parent2 = new LogTapeLogger(
            sharedLt as unknown as LogTapeLoggerImpl,
        );

        const child1a = parent1.child("sub");
        const child1b = parent1.child("sub");
        const child2a = parent2.child("sub");
        const child2b = parent2.child("sub");

        // Each parent's child cache is internally consistent:
        expect(child1a).toBe(child1b);
        expect(child2a).toBe(child2b);
        // But the two parents do not share child wrappers, even when the
        // underlying child ltLogger is identical:
        expect(child1a).not.toBe(child2a);

        // Each cached child still carries the correct parent reference:
        expect(child1a.parent).toBe(parent1);
        expect(child2a.parent).toBe(parent2);

        // Both cache slots co-exist on the same child ltLogger:
        const childCacheSymbols = Object.getOwnPropertySymbols(childLt).filter(
            (s) => s.description === "LogTapeLoggerFactory.cache",
        );
        expect(childCacheSymbols).toHaveLength(2);
    });
});

describe("LogTapeLogger.with", () => {
    it("delegates to ltLogger.with with the supplied properties", () => {
        const { logger, mock } = makeLogger();
        mock.with.mockReturnValue(createMockLtLogger());
        const properties = { user: "alice", count: 3 };

        logger.with(properties);

        expect(mock.with).toHaveBeenCalledTimes(1);
        expect(mock.with).toHaveBeenCalledWith(properties);
    });

    it("forwards the properties object by reference (no clone)", () => {
        const { logger, mock } = makeLogger();
        mock.with.mockReturnValue(createMockLtLogger());
        const properties = { user: "alice" };

        logger.with(properties);

        expect(mock.with.mock.calls[0]?.[0]).toBe(properties);
    });

    it("supports an empty properties object", () => {
        const { logger, mock } = makeLogger();
        mock.with.mockReturnValue(createMockLtLogger());

        logger.with({});

        expect(mock.with).toHaveBeenCalledTimes(1);
        expect(mock.with).toHaveBeenCalledWith({});
    });

    it("returns a LogTapeLogger wrapping the contextual ltLogger", () => {
        const { logger, mock } = makeLogger();
        const ctxLt = createMockLtLogger();
        mock.with.mockReturnValue(ctxLt);

        const ctx = logger.with({ user: "alice" });

        expect(ctx).toBeInstanceOf(LogTapeLogger);
        expect(ctx).not.toBe(logger);
    });

    it("contextual logger's parent reflects the underlying ltLogger.parent (logtape LoggerCtx semantics)", () => {
        // logtape's `LoggerCtx.parent` returns `this.logger.parent`, i.e.
        // the parent of the *underlying* logger — NOT the wrapper that
        // called `.with(...)`. Our dynamic getter mirrors that exactly.
        const { logger, mock } = makeLogger();
        const ancestorMock = createMockLtLogger();
        mock.parent = ancestorMock as unknown as LogTapeLoggerImpl;

        const ctxLt = createMockLtLogger();
        // Logtape's LoggerCtx delegates `parent` to the underlying logger,
        // so the ctxLt's parent is the same as `mock.parent`:
        ctxLt.parent = ancestorMock as unknown as LogTapeLoggerImpl;
        mock.with.mockReturnValue(ctxLt);

        const ctx = logger.with({ user: "alice" });

        // ctx.parent === logger.parent (both resolve via cache to the same
        // wrapper around `ancestorMock`).
        expect(ctx.parent).toBe(logger.parent);
        expect(ctx.parent).not.toBe(logger);
    });

    it("uses the contextual ltLogger's surface for subsequent log calls", () => {
        const { logger, mock } = makeLogger();
        const ctxLt = createMockLtLogger();
        mock.with.mockReturnValue(ctxLt);

        const ctx = logger.with({ user: "alice" });
        ctx.info("hello");

        // The contextual logger must route to the value returned by
        // ltLogger.with, not the parent ltLogger.
        expect(ctxLt.info).toHaveBeenCalledTimes(1);
        expect(ctxLt.info).toHaveBeenCalledWith("hello", undefined);
        expect(mock.info).not.toHaveBeenCalled();
    });

    it("does not invoke any log/level methods on the original ltLogger", () => {
        const { logger, mock } = makeLogger();
        mock.with.mockReturnValue(createMockLtLogger());

        logger.with({ user: "alice" });

        // `with` is purely a contextual constructor: it should not log
        // anything or query enabled state on the original logger.
        expect(mock.info).not.toHaveBeenCalled();
        expect(mock.warning).not.toHaveBeenCalled();
        expect(mock.debug).not.toHaveBeenCalled();
        expect(mock.error).not.toHaveBeenCalled();
        expect(mock.trace).not.toHaveBeenCalled();
        expect(mock.isEnabledFor).not.toHaveBeenCalled();
    });

    it("each call to with() produces a fresh LogTapeLogger", () => {
        const { logger, mock } = makeLogger();
        mock.with.mockReturnValueOnce(createMockLtLogger());
        mock.with.mockReturnValueOnce(createMockLtLogger());

        const a = logger.with({ user: "alice" });
        const b = logger.with({ user: "alice" });

        expect(a).not.toBe(b);
        expect(mock.with).toHaveBeenCalledTimes(2);
    });

    it("supports chaining: with().with() goes through ltLogger.with twice", () => {
        const { logger, mock } = makeLogger();
        const firstCtx = createMockLtLogger();
        const secondCtx = createMockLtLogger();
        mock.with.mockReturnValue(firstCtx);
        firstCtx.with.mockReturnValue(secondCtx);

        const first = logger.with({ a: 1 });
        // Drive a second `with` to verify the call chain.
        first.with({ b: 2 });

        expect(mock.with).toHaveBeenCalledTimes(1);
        expect(mock.with).toHaveBeenCalledWith({ a: 1 });
        expect(firstCtx.with).toHaveBeenCalledTimes(1);
        expect(firstCtx.with).toHaveBeenCalledWith({ b: 2 });

        // Parent linkage is now driven by ltLogger.parent (see the
        // "contextual logger's parent reflects the underlying ltLogger.parent"
        // test); chaining `with` calls only verifies the dispatch path.
    });

    it("the contextual logger preserves the LogTapeLogger surface (child, with, enabled, leveled methods)", () => {
        const { logger, mock } = makeLogger();
        const ctxLt = createMockLtLogger();
        ctxLt.isEnabledFor.mockReturnValue(true);
        mock.with.mockReturnValue(ctxLt);

        const ctx = logger.with({ user: "alice" });

        expect(typeof ctx.child).toBe("function");
        expect(typeof ctx.with).toBe("function");
        expect(typeof ctx.enabled).toBe("function");
        expect(typeof ctx.info).toBe("function");
        expect(typeof ctx.warn).toBe("function");
        expect(typeof ctx.error).toBe("function");
        expect(typeof ctx.debug).toBe("function");
        expect(typeof ctx.trace).toBe("function");
        expect(ctx.enabled("info")).toBe(true);
        expect(ctxLt.isEnabledFor).toHaveBeenCalledWith("info");
    });
});

describe("LogTapeLogger.logEager", () => {
    it("calls the matching logtape method with (message, properties) for each level", () => {
        for (const level of LOG_LEVELS) {
            const { logger, mock } = makeLogger();
            const properties = { user: "alice", lvl: level };

            logger.logEager(level, `msg-${level}`, properties);

            const ltMethod = LEVEL_TO_LT_METHOD[level];
            expect(mock[ltMethod]).toHaveBeenCalledTimes(1);
            expect(mock[ltMethod]).toHaveBeenCalledWith(
                `msg-${level}`,
                properties,
            );

            // Other level methods must remain untouched.
            for (const other of LOG_LEVELS) {
                if (other === level) continue;
                expect(mock[LEVEL_TO_LT_METHOD[other]]).not.toHaveBeenCalled();
            }
        }
    });

    it("forwards undefined properties as-is", () => {
        const { logger, mock } = makeLogger();

        logger.logEager("info", "hello");

        expect(mock.info).toHaveBeenCalledWith("hello", undefined);
    });

    it("routes 'warn' to logtape 'warning'", () => {
        const { logger, mock } = makeLogger();
        const properties = { x: 1 };

        logger.logEager("warn", "careful", properties);

        expect(mock.warning).toHaveBeenCalledTimes(1);
        expect(mock.warning).toHaveBeenCalledWith("careful", properties);
    });
});

describe("LogTapeLogger.logLazy", () => {
    it("forwards the compute function reference without invoking it", () => {
        for (const level of LOG_LEVELS) {
            const { logger, mock } = makeLogger();
            const compute = vi.fn(() => ({ a: 1 }));

            logger.logLazy(level, `msg-${level}`, compute);

            const ltMethod = LEVEL_TO_LT_METHOD[level];
            expect(mock[ltMethod]).toHaveBeenCalledTimes(1);
            expect(mock[ltMethod]).toHaveBeenCalledWith(
                `msg-${level}`,
                compute,
            );
            // Same reference, not a wrapper.
            expect(mock[ltMethod].mock.calls[0]?.[1]).toBe(compute);
            // Lazy: dispatch must never invoke the compute fn.
            expect(compute).not.toHaveBeenCalled();
        }
    });

    it("forwards undefined when no compute fn is given", () => {
        const { logger, mock } = makeLogger();

        logger.logLazy("info", "hello");

        expect(mock.info).toHaveBeenCalledWith("hello", undefined);
    });

    it("routes 'warn' to logtape 'warning'", () => {
        const { logger, mock } = makeLogger();
        const compute = () => ({ x: 1 });

        logger.logLazy("warn", "careful", compute);

        expect(mock.warning).toHaveBeenCalledWith("careful", compute);
    });
});

describe("LogTapeLogger.logLazyAsync", () => {
    it("returns the Promise produced by the underlying logtape method", async () => {
        const { logger, mock } = makeLogger();
        const sinkPromise = Promise.resolve();
        mock.info.mockReturnValue(sinkPromise);

        const compute = async () => ({ a: 1 });
        const result = logger.logLazyAsync("info", "msg", compute);

        expect(result).toBe(sinkPromise);
        expect(mock.info).toHaveBeenCalledTimes(1);
        expect(mock.info).toHaveBeenCalledWith("msg", compute);
        await result;
    });

    it("forwards the async compute reference without invoking it", async () => {
        const { logger, mock } = makeLogger();
        let invoked = false;
        const compute = async () => {
            invoked = true;
            return { a: 1 };
        };
        mock.info.mockReturnValue(Promise.resolve());

        await logger.logLazyAsync("info", "msg", compute);

        expect(mock.info.mock.calls[0]?.[1]).toBe(compute);
        expect(invoked).toBe(false);
    });

    it("propagates rejections from the underlying logtape method", async () => {
        const { logger, mock } = makeLogger();
        const failure = new Error("sink failed");
        mock.info.mockReturnValue(Promise.reject(failure));

        const compute = async () => ({ a: 1 });
        await expect(logger.logLazyAsync("info", "msg", compute)).rejects.toBe(
            failure,
        );
    });

    it("routes async compute to the matching logtape method for each level", async () => {
        for (const level of LOG_LEVELS) {
            const { logger, mock } = makeLogger();
            const ltMethod = LEVEL_TO_LT_METHOD[level];
            mock[ltMethod].mockReturnValue(Promise.resolve());
            const compute = async () => ({ lvl: level });

            await logger.logLazyAsync(level, `msg-${level}`, compute);

            expect(mock[ltMethod]).toHaveBeenCalledTimes(1);
            expect(mock[ltMethod]).toHaveBeenCalledWith(
                `msg-${level}`,
                compute,
            );
        }
    });

    it("routes 'warn' async compute to logtape 'warning'", async () => {
        const { logger, mock } = makeLogger();
        mock.warning.mockReturnValue(Promise.resolve());
        const compute = async () => ({ x: 1 });

        await logger.logLazyAsync("warn", "careful", compute);

        expect(mock.warning).toHaveBeenCalledWith("careful", compute);
    });
});

describe("LogTapeLogger.logTemplate", () => {
    it("forwards (template, ...args) spread into the matching logtape method for each level", () => {
        for (const level of LOG_LEVELS) {
            const { logger, mock } = makeLogger();
            const tpl = makeTemplate(["a ", " b ", ""]);
            const args = [1, "two"];

            logger.logTemplate(level, tpl, args);

            const ltMethod = LEVEL_TO_LT_METHOD[level];
            // logtape's tagged-template signature is (template, ...values),
            // so each interpolation must arrive as a separate argument.
            expect(mock[ltMethod]).toHaveBeenCalledTimes(1);
            expect(mock[ltMethod]).toHaveBeenCalledWith(tpl, 1, "two");
            expect(mock[ltMethod].mock.calls[0]?.[0]).toBe(tpl);
            expect(mock[ltMethod].mock.calls[0]?.[1]).toBe(1);
            expect(mock[ltMethod].mock.calls[0]?.[2]).toBe("two");
        }
    });

    it("routes 'warn' templates to logtape 'warning'", () => {
        const { logger, mock } = makeLogger();
        const tpl = makeTemplate(["x ", ""]);

        logger.logTemplate("warn", tpl, ["y"]);

        expect(mock.warning).toHaveBeenCalledTimes(1);
        expect(mock.warning).toHaveBeenCalledWith(tpl, "y");
    });

    it("supports empty template/args arrays", () => {
        const { logger, mock } = makeLogger();
        const tpl = makeTemplate(["just a static string"]);

        logger.logTemplate("info", tpl, []);

        // No interpolations: only the template is forwarded.
        expect(mock.info).toHaveBeenCalledWith(tpl);
    });

    it("forwards the same TemplateStringsArray reference (no copy)", () => {
        const { logger, mock } = makeLogger();
        const tpl = makeTemplate(["a ", ""]);

        logger.logTemplate("info", tpl, [42]);

        expect(mock.info.mock.calls[0]?.[0]).toBe(tpl);
    });
});

describe("LogTapeLogger logFn error path", () => {
    it("throws when the underlying ltLogger does not expose the level method", () => {
        const partial = createMockLtLogger() as unknown as Record<
            string,
            unknown
        >;
        // Simulate a logtape logger that's missing the `info` method.
        partial.info = undefined;
        const logger = new LogTapeLogger(
            partial as unknown as LogTapeLoggerImpl,
        );

        expect(() => logger.logEager("info", "hello")).toThrow(
            /Unsupported log level: info/,
        );
    });

    it("uses the converted level name in the error path", () => {
        const partial = createMockLtLogger() as unknown as Record<
            string,
            unknown
        >;
        // Even though our level is "warn", the lookup uses "warning".
        partial.warning = undefined;
        const logger = new LogTapeLogger(
            partial as unknown as LogTapeLoggerImpl,
        );

        // The thrown message reports the *application* level, not the
        // logtape level. We only assert that an error is thrown when the
        // converted name is missing.
        expect(() => logger.logEager("warn", "careful")).toThrow(
            /Unsupported log level: warn/,
        );
    });
});

describe("LogTapeLogger end-to-end through AbstractLogger dispatch", () => {
    it("logger.info(message, properties) -> ltLogger.info(message, properties)", () => {
        const { logger, mock } = makeLogger();
        const properties = { a: 1 };

        logger.info("hello", properties);

        expect(mock.info).toHaveBeenCalledTimes(1);
        expect(mock.info).toHaveBeenCalledWith("hello", properties);
    });

    it("logger.info(message, syncCompute) -> ltLogger.info(message, syncCompute)", () => {
        const { logger, mock } = makeLogger();
        const compute = () => ({ a: 1 });

        logger.info("hello", compute);

        expect(mock.info).toHaveBeenCalledTimes(1);
        expect(mock.info).toHaveBeenCalledWith("hello", compute);
        expect(mock.info.mock.calls[0]?.[1]).toBe(compute);
    });

    it("logger.info(message, asyncCompute) -> Promise from ltLogger.info(message, asyncCompute)", async () => {
        const { logger, mock } = makeLogger();
        const sinkPromise = Promise.resolve();
        mock.info.mockReturnValue(sinkPromise);
        const compute = async () => ({ a: 1 });

        const result = logger.info("hello", compute);

        expect(result).toBe(sinkPromise);
        expect(mock.info).toHaveBeenCalledWith("hello", compute);
        await result;
    });

    it("logger.info`tagged [value]` -> ltLogger.info(template, ...values) (spread)", () => {
        const { logger, mock } = makeLogger();

        const value = "world";
        logger.info`tagged ${value}`;

        expect(mock.info).toHaveBeenCalledTimes(1);
        const call = mock.info.mock.calls[0];
        expect(call).toBeDefined();
        // (template, ...values) — `value` arrives in its own slot, not nested.
        const [template, firstValue, ...rest] = call as [
            TemplateStringsArray,
            ...unknown[],
        ];
        expect([...template]).toEqual(["tagged ", ""]);
        expect(firstValue).toBe(value);
        expect(rest).toEqual([]);
    });

    it("logger.warn routes to ltLogger.warning (not ltLogger.warn)", () => {
        const { logger, mock } = makeLogger();

        logger.warn("careful", { x: 1 });

        expect(mock.warning).toHaveBeenCalledTimes(1);
        expect(mock.warning).toHaveBeenCalledWith("careful", { x: 1 });
    });

    it("logger.log(level, message, properties) routes via the AbstractLogger overload", () => {
        const { logger, mock } = makeLogger();

        logger.log("debug", "hi", { a: 1 });

        expect(mock.debug).toHaveBeenCalledTimes(1);
        expect(mock.debug).toHaveBeenCalledWith("hi", { a: 1 });
    });

    it("destructured leveled methods preserve `this` binding", () => {
        const { logger, mock } = makeLogger();

        const { error, info } = logger;
        error`boom ${1}`;
        info("hi");

        expect(mock.error).toHaveBeenCalledTimes(1);
        expect(mock.info).toHaveBeenCalledTimes(1);
    });
});

describe("LogTapeLoggerFactory", () => {
    const getLoggerMock = vi.mocked(getLogger);

    beforeEach(() => {
        getLoggerMock.mockReset();
    });

    afterEach(() => {
        getLoggerMock.mockReset();
    });

    it("calls getLogger with the supplied category", () => {
        const ltLogger = createMockLtLogger();
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const factory = new LogTapeLoggerFactory();
        factory.get("category");

        expect(getLoggerMock).toHaveBeenCalledTimes(1);
        expect(getLoggerMock).toHaveBeenCalledWith("category");
    });

    it("supports tuple categories", () => {
        const ltLogger = createMockLtLogger();
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const factory = new LogTapeLoggerFactory();
        factory.get(["app", "auth"]);

        expect(getLoggerMock).toHaveBeenCalledWith(["app", "auth"]);
    });

    it("returns a LogTapeLogger wrapping the result of getLogger", () => {
        const ltLogger = createMockLtLogger();
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const factory = new LogTapeLoggerFactory();
        const result = factory.get("category");

        expect(result).toBeInstanceOf(LogTapeLogger);
    });

    it("the produced logger forwards calls to the wrapped ltLogger", () => {
        const ltLogger = createMockLtLogger();
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const factory = new LogTapeLoggerFactory();
        const result = factory.get("category");
        result.info("hello", { a: 1 });

        expect(ltLogger.info).toHaveBeenCalledTimes(1);
        expect(ltLogger.info).toHaveBeenCalledWith("hello", { a: 1 });
    });

    it("the produced logger has a null parent (top-level)", () => {
        const ltLogger = createMockLtLogger();
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const factory = new LogTapeLoggerFactory();
        const result = factory.get("category");

        expect(result.parent).toBeNull();
    });

    it("caches the wrapper per category: repeated calls return the same instance", () => {
        const ltLogger = createMockLtLogger();
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const factory = new LogTapeLoggerFactory();
        const a = factory.get("category");
        const b = factory.get("category");

        expect(a).toBe(b);
    });

    it("only constructs a single LogTapeLogger wrapper for repeated get() calls with the same category", () => {
        const ltLogger = createMockLtLogger();
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const factory = new LogTapeLoggerFactory();
        factory.get("category");
        factory.get("category");
        factory.get("category");

        // The wrapper is attached as a Symbol-keyed property on the
        // logtape Logger object. Exactly one of those Symbols must exist
        // after multiple gets, no matter how many times we asked.
        const factorySymbols = Object.getOwnPropertySymbols(ltLogger).filter(
            (s) => s.description === "LogTapeLoggerFactory.cache",
        );
        expect(factorySymbols).toHaveLength(1);
    });

    it("returns different wrappers for different categories", () => {
        const ltA = createMockLtLogger();
        const ltB = createMockLtLogger();
        getLoggerMock.mockImplementation(((category: unknown) => {
            const head = Array.isArray(category)
                ? category[0]
                : (category as string);
            return (head === "a" ? ltA : ltB) as unknown as LogTapeLoggerImpl;
        }) as unknown as typeof getLogger);

        const factory = new LogTapeLoggerFactory();
        const a = factory.get(["a"]);
        const b = factory.get(["b"]);

        expect(a).not.toBe(b);
    });

    it("two factory instances maintain independent caches (same logtape logger, different wrappers)", () => {
        const ltLogger = createMockLtLogger();
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const f1 = new LogTapeLoggerFactory();
        const f2 = new LogTapeLoggerFactory();

        const wrapper1a = f1.get("category");
        const wrapper1b = f1.get("category");
        const wrapper2a = f2.get("category");
        const wrapper2b = f2.get("category");

        // Each factory caches its own wrapper:
        expect(wrapper1a).toBe(wrapper1b);
        expect(wrapper2a).toBe(wrapper2b);
        // But the two factories never share wrappers, even when the
        // underlying logtape logger is identical:
        expect(wrapper1a).not.toBe(wrapper2a);

        // The two distinct cache slots co-exist on the same ltLogger:
        const factorySymbols = Object.getOwnPropertySymbols(ltLogger).filter(
            (s) => s.description === "LogTapeLoggerFactory.cache",
        );
        expect(factorySymbols).toHaveLength(2);
    });

    it("propagates the factory's cache key to descendants: child wrappers and direct factory.get of a deep category share the same slot", () => {
        // Two distinct logtape loggers: one for the root category, one for
        // the child category. Both `factory.get([root, child])` and
        // `root.child(child)` resolve to the same logtape Logger
        // (`childLt`), since logtape's getLogger and getChild are
        // internally consistent.
        const rootLt = createMockLtLogger();
        const childLt = createMockLtLogger();
        rootLt.getChild.mockReturnValue(childLt);
        getLoggerMock.mockImplementation(((category: unknown) => {
            const arr = Array.isArray(category)
                ? category
                : [category as string];
            if (arr.length === 1 && arr[0] === "app") {
                return rootLt as unknown as LogTapeLoggerImpl;
            }
            // For ["app", "auth"] (or similar), return the same `childLt`
            // that `rootLt.getChild(...)` would return.
            return childLt as unknown as LogTapeLoggerImpl;
        }) as unknown as typeof getLogger);

        const factory = new LogTapeLoggerFactory();
        const viaChain = factory.get(["app"]).child("auth");
        const viaDirect = factory.get(["app", "auth"]);

        // Both routes hit the same cache slot on `childLt`, so the wrapper
        // is identical no matter which path was taken first.
        expect(viaChain).toBe(viaDirect);

        // And only one cache slot is used — the factory's Symbol — even
        // though we reached the same logtape Logger through two different
        // routes.
        const symbolsOnChild = Object.getOwnPropertySymbols(childLt).filter(
            (s) => s.description === "LogTapeLoggerFactory.cache",
        );
        expect(symbolsOnChild).toHaveLength(1);

        // Cross-check: the very same Symbol is used on the root logtape
        // Logger (i.e. it really is the one factory key, threaded
        // throughout the whole family).
        const symbolsOnRoot = Object.getOwnPropertySymbols(rootLt).filter(
            (s) => s.description === "LogTapeLoggerFactory.cache",
        );
        expect(symbolsOnRoot).toHaveLength(1);
        expect(symbolsOnChild[0]).toBe(symbolsOnRoot[0]);
    });

    it("a wrapper constructed directly (without a factory) gets its own default Symbol, isolated from any factory", () => {
        const ltLogger = createMockLtLogger();
        const childLt = createMockLtLogger();
        ltLogger.getChild.mockReturnValue(childLt);
        getLoggerMock.mockReturnValue(ltLogger as unknown as LogTapeLoggerImpl);

        const factory = new LogTapeLoggerFactory();
        const fromFactory = factory.get("category");
        const standalone = new LogTapeLogger(
            ltLogger as unknown as LogTapeLoggerImpl,
        );

        // Different wrapper instances:
        expect(fromFactory).not.toBe(standalone);

        // Drive a `.child()` call from each so both Symbols actually get
        // attached to the same child ltLogger — only then can we observe
        // them side-by-side.
        const factoryChild = fromFactory.child("sub");
        const standaloneChild = standalone.child("sub");
        expect(factoryChild).not.toBe(standaloneChild);

        // The factory's Symbol and the standalone wrapper's default Symbol
        // co-exist on the child ltLogger — they are *different* Symbol
        // values even though both share the same description.
        const symbols = Object.getOwnPropertySymbols(childLt).filter(
            (s) => s.description === "LogTapeLoggerFactory.cache",
        );
        expect(symbols).toHaveLength(2);
        expect(symbols[0]).not.toBe(symbols[1]);
    });
});

/**
 * Construct a value structurally equivalent to a real
 * TemplateStringsArray (a frozen array with a `raw` property), so the
 * mocked logtape methods receive something indistinguishable from a
 * real tagged template invocation.
 */
function makeTemplate(strings: readonly string[]): TemplateStringsArray {
    const arr = [...strings] as string[] & { raw: readonly string[] };
    arr.raw = [...strings];
    Object.freeze(arr.raw);
    Object.freeze(arr);
    return arr as unknown as TemplateStringsArray;
}
