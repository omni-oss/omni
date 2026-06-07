import { describe, expect, it, vi } from "vitest";
import { LOG_LEVELS, type LogLevel } from "./level";
import {
    AbstractLogger,
    type CategoryParam,
    type LogEager,
    type Logger,
    type LogLazy,
    type LogLazyAsync,
    type LogTemplate,
} from "./logger";

class TestLogger extends AbstractLogger {
    private _parent: Logger | null = null;

    constructor(parent?: Logger) {
        super();
        this._parent = parent ?? null;
    }

    logEager = vi.fn<LogEager>();

    logLazy = vi.fn<LogLazy>();

    logLazyAsync = vi.fn<LogLazyAsync>().mockImplementation(async () => {
        /* default: resolve immediately */
    });

    logTemplate = vi.fn<LogTemplate>();

    enabled = vi.fn((_: LogLevel) => {
        // For testing purposes, all levels are enabled by default.
        return true;
    });

    child = vi.fn((_categories: CategoryParam) => {
        // For testing purposes, return the same logger instance for any child.
        return new TestLogger(this);
    });

    get parent() {
        return this._parent;
    }

    with = vi.fn((_properties: Record<string, unknown>) => {
        // For testing purposes, return the same logger instance for any with().
        return new TestLogger(this);
    });
}

describe("AbstractLogger.log", () => {
    it("dispatches to logEager when called with (level, message)", () => {
        const logger = new TestLogger();

        logger.log("info", "hello");

        expect(logger.logEager).toHaveBeenCalledTimes(1);
        expect(logger.logEager).toHaveBeenCalledWith(
            "info",
            "hello",
            undefined,
        );
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("dispatches to logEager with the properties object when called with (level, message, properties)", () => {
        const logger = new TestLogger();
        const properties = { user: "alice", count: 3 };

        logger.log("warn", "did the thing", properties);

        expect(logger.logEager).toHaveBeenCalledTimes(1);
        expect(logger.logEager).toHaveBeenCalledWith(
            "warn",
            "did the thing",
            properties,
        );
        expect(logger.logEager.mock.calls[0]?.[2]).toBe(properties);
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("dispatches to logLazy when properties argument is a function", () => {
        const logger = new TestLogger();
        const compute = () => ({ a: 1 });

        logger.log("debug", "compute me", compute);

        expect(logger.logLazy).toHaveBeenCalledTimes(1);
        expect(logger.logLazy).toHaveBeenCalledWith(
            "debug",
            "compute me",
            compute,
        );
        expect(logger.logLazy.mock.calls[0]?.[2]).toBe(compute);
        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("returns a leveled log function when called with only a level", () => {
        const logger = new TestLogger();

        const leveled = logger.log("error");

        expect(typeof leveled).toBe("function");
        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("returns the same reference as the matching leveled method", () => {
        const logger = new TestLogger();

        expect(logger.log("error")).toBe(logger.error);
        expect(logger.log("warn")).toBe(logger.warn);
        expect(logger.log("info")).toBe(logger.info);
        expect(logger.log("debug")).toBe(logger.debug);
        expect(logger.log("trace")).toBe(logger.trace);
    });

    it("returns void (undefined) when invoked with a message and no compute", () => {
        const logger = new TestLogger();

        const result = logger.log("info", "hi");

        expect(result).toBeUndefined();
    });

    it("returns void (undefined) when invoked with a synchronous compute function", () => {
        const logger = new TestLogger();

        const result = logger.log("info", "hi", () => ({ a: 1 }));

        expect(result).toBeUndefined();
        expect(logger.logLazy).toHaveBeenCalledTimes(1);
        expect(logger.logLazyAsync).not.toHaveBeenCalled();
    });

    it("the leveled function returned by log(level) routes a tagged template to logTemplate", () => {
        const logger = new TestLogger();
        const leveled = logger.log("trace");

        const name = "world";
        const count = 42;
        leveled`hello ${name} count=${count}`;

        expect(logger.logTemplate).toHaveBeenCalledTimes(1);

        const call = logger.logTemplate.mock.calls[0];
        expect(call).toBeDefined();
        const [level, template, args] = call as [
            LogLevel,
            TemplateStringsArray,
            unknown[],
        ];
        expect(level).toBe("trace");
        expect(Array.isArray(template)).toBe(true);
        expect([...template]).toEqual(["hello ", " count=", ""]);
        expect(template.raw).toBeDefined();
        expect(args).toEqual([name, count]);

        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logLazy).not.toHaveBeenCalled();
    });

    it("the leveled function returned by log(level) routes (message) to logEager", () => {
        const logger = new TestLogger();
        const leveled = logger.log("info");

        leveled("hello");

        expect(logger.logEager).toHaveBeenCalledTimes(1);
        expect(logger.logEager).toHaveBeenCalledWith(
            "info",
            "hello",
            undefined,
        );
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("the leveled function returned by log(level) routes (message, properties) to logEager", () => {
        const logger = new TestLogger();
        const leveled = logger.log("warn");
        const properties = { user: "alice" };

        leveled("did the thing", properties);

        expect(logger.logEager).toHaveBeenCalledTimes(1);
        expect(logger.logEager).toHaveBeenCalledWith(
            "warn",
            "did the thing",
            properties,
        );
        expect(logger.logEager.mock.calls[0]?.[2]).toBe(properties);
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("the leveled function returned by log(level) routes (message, computeFn) to logLazy", () => {
        const logger = new TestLogger();
        const leveled = logger.log("debug");
        const compute = vi.fn(() => ({ a: 1 }));

        leveled("compute me", compute);

        expect(logger.logLazy).toHaveBeenCalledTimes(1);
        expect(logger.logLazy).toHaveBeenCalledWith(
            "debug",
            "compute me",
            compute,
        );
        expect(logger.logLazy.mock.calls[0]?.[2]).toBe(compute);
        expect(compute).not.toHaveBeenCalled();
        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("the leveled function can be invoked multiple times, dispatching each time", () => {
        const logger = new TestLogger();
        const leveled = logger.log("info");

        leveled`first ${1}`;
        leveled`second ${"two"}`;

        expect(logger.logTemplate).toHaveBeenCalledTimes(2);
        expect(logger.logTemplate.mock.calls[0]?.[0]).toBe("info");
        expect(logger.logTemplate.mock.calls[0]?.[2]).toEqual([1]);
        expect(logger.logTemplate.mock.calls[1]?.[0]).toBe("info");
        expect(logger.logTemplate.mock.calls[1]?.[2]).toEqual(["two"]);
    });

    it("caches the leveled function per level (repeated log(level) calls return the same reference)", () => {
        const logger = new TestLogger();

        for (const level of LOG_LEVELS) {
            const a = logger.log(level);
            const b = logger.log(level);
            expect(a).toBe(b);
        }

        const seen = new Set<unknown>();
        for (const level of LOG_LEVELS) {
            seen.add(logger.log(level));
        }
        expect(seen.size).toBe(LOG_LEVELS.length);
    });

    it("each logger instance owns its own cached leveled functions", () => {
        const a = new TestLogger();
        const b = new TestLogger();

        expect(a.log("info")).not.toBe(b.log("info"));
    });

    it("supports every defined LogLevel via log(level, message)", () => {
        const logger = new TestLogger();

        for (const level of LOG_LEVELS) {
            logger.log(level, `msg-${level}`);
        }

        expect(logger.logEager).toHaveBeenCalledTimes(LOG_LEVELS.length);
        for (const [i, level] of LOG_LEVELS.entries()) {
            expect(logger.logEager.mock.calls[i]?.[0]).toBe(level);
            expect(logger.logEager.mock.calls[i]?.[1]).toBe(`msg-${level}`);
        }
    });

    it("supports every defined LogLevel via log(level)`...`", () => {
        const logger = new TestLogger();

        for (const level of LOG_LEVELS) {
            logger.log(level)`tagged ${level}`;
        }

        expect(logger.logTemplate).toHaveBeenCalledTimes(LOG_LEVELS.length);
        for (const [i, level] of LOG_LEVELS.entries()) {
            expect(logger.logTemplate.mock.calls[i]?.[0]).toBe(level);
            expect(logger.logTemplate.mock.calls[i]?.[2]).toEqual([level]);
        }
    });

    it("preserves `this` so detached `log` still dispatches correctly (arrow-bound)", () => {
        const logger = new TestLogger();

        const detached = logger.log;
        detached("info", "hello", { x: 1 });

        expect(logger.logEager).toHaveBeenCalledTimes(1);
        expect(logger.logEager).toHaveBeenCalledWith("info", "hello", { x: 1 });
    });

    it("does not call logLazy when properties is null (nullish, not a function)", () => {
        const logger = new TestLogger();

        logger.log("info", "msg", null as unknown as Record<string, unknown>);

        expect(logger.logEager).toHaveBeenCalledTimes(1);
        expect(logger.logEager).toHaveBeenCalledWith("info", "msg", null);
        expect(logger.logLazy).not.toHaveBeenCalled();
    });

    it("throws TypeError when message is undefined but extra args are supplied", () => {
        const logger = new TestLogger();

        expect(() =>
            logger.log(
                "info",
                undefined as unknown as string,
                { a: 1 } as Record<string, unknown>,
            ),
        ).toThrow(TypeError);
        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("throws TypeError when message is null", () => {
        const logger = new TestLogger();

        expect(() => logger.log("info", null as unknown as string)).toThrow(
            TypeError,
        );
        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });
});

describe("AbstractLogger leveled methods", () => {
    const LEVEL_TO_METHOD: Record<
        Exclude<LogLevel, never>,
        "error" | "warn" | "info" | "debug" | "trace"
    > = {
        error: "error",
        warn: "warn",
        info: "info",
        debug: "debug",
        trace: "trace",
    };

    it("exposes a function for every supported level", () => {
        const logger = new TestLogger();

        for (const level of LOG_LEVELS) {
            const method = LEVEL_TO_METHOD[level];
            expect(typeof logger[method]).toBe("function");
        }
    });

    it("routes tagged-template calls to logTemplate with the matching level", () => {
        for (const level of LOG_LEVELS) {
            const logger = new TestLogger();
            const method = LEVEL_TO_METHOD[level];

            const x = "value";
            logger[method]`tagged ${x}`;

            expect(logger.logTemplate).toHaveBeenCalledTimes(1);
            const call = logger.logTemplate.mock.calls[0];
            expect(call).toBeDefined();
            const [calledLevel, template, args] = call as [
                LogLevel,
                TemplateStringsArray,
                unknown[],
            ];
            expect(calledLevel).toBe(level);
            expect([...template]).toEqual(["tagged ", ""]);
            expect(args).toEqual([x]);

            expect(logger.logEager).not.toHaveBeenCalled();
            expect(logger.logLazy).not.toHaveBeenCalled();
        }
    });

    it("routes (message) calls to logEager with undefined properties and the matching level", () => {
        for (const level of LOG_LEVELS) {
            const logger = new TestLogger();
            const method = LEVEL_TO_METHOD[level];

            logger[method](`hello ${level}`);

            expect(logger.logEager).toHaveBeenCalledTimes(1);
            expect(logger.logEager).toHaveBeenCalledWith(
                level,
                `hello ${level}`,
                undefined,
            );
            expect(logger.logTemplate).not.toHaveBeenCalled();
            expect(logger.logLazy).not.toHaveBeenCalled();
        }
    });

    it("routes (message, properties) calls to logEager with the unwrapped properties object", () => {
        for (const level of LOG_LEVELS) {
            const logger = new TestLogger();
            const method = LEVEL_TO_METHOD[level];
            const properties = { user: "alice", lvl: level };

            logger[method](`hello ${level}`, properties);

            expect(logger.logEager).toHaveBeenCalledTimes(1);
            expect(logger.logEager).toHaveBeenCalledWith(
                level,
                `hello ${level}`,
                properties,
            );
            expect(logger.logEager.mock.calls[0]?.[2]).toBe(properties);
            expect(logger.logTemplate).not.toHaveBeenCalled();
            expect(logger.logLazy).not.toHaveBeenCalled();
        }
    });

    it("routes (message, computeFn) calls to logLazy with the function reference", () => {
        for (const level of LOG_LEVELS) {
            const logger = new TestLogger();
            const method = LEVEL_TO_METHOD[level];
            const compute = vi.fn(() => ({ user: "alice", lvl: level }));

            logger[method](`hello ${level}`, compute);

            expect(logger.logLazy).toHaveBeenCalledTimes(1);
            expect(logger.logLazy).toHaveBeenCalledWith(
                level,
                `hello ${level}`,
                compute,
            );
            expect(logger.logLazy.mock.calls[0]?.[2]).toBe(compute);
            expect(compute).not.toHaveBeenCalled();
            expect(logger.logEager).not.toHaveBeenCalled();
            expect(logger.logTemplate).not.toHaveBeenCalled();
        }
    });

    it("does not invoke any sink at construction time", () => {
        const logger = new TestLogger();

        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("methods can be destructured / detached and still dispatch correctly (arrow-bound)", () => {
        const logger = new TestLogger();

        const { error, warn, info, debug, trace } = logger;

        error`a ${1}`;
        warn`b ${2}`;
        info`c ${3}`;
        debug`d ${4}`;
        trace`e ${5}`;

        expect(logger.logTemplate).toHaveBeenCalledTimes(5);
        expect(logger.logTemplate.mock.calls.map((c) => c[0])).toEqual([
            "error",
            "warn",
            "info",
            "debug",
            "trace",
        ]);
        expect(logger.logTemplate.mock.calls.map((c) => c[2])).toEqual([
            [1],
            [2],
            [3],
            [4],
            [5],
        ]);
    });

    it("each level method only writes to its own level (no cross-talk)", () => {
        const logger = new TestLogger();

        logger.error`only error`;
        logger.warn`only warn`;

        const levels = logger.logTemplate.mock.calls.map((c) => c[0]);
        expect(levels).toEqual(["error", "warn"]);
    });

    it("invokes the abstract sink on the same instance (correct `this`)", () => {
        const logger = new TestLogger();

        const { info } = logger;
        info`x ${"y"}`;

        expect(logger.logTemplate).toHaveBeenCalledTimes(1);
    });

    it("supports calls with no interpolations in tagged templates", () => {
        const logger = new TestLogger();

        logger.info`just a static string`;

        expect(logger.logTemplate).toHaveBeenCalledTimes(1);
        const call = logger.logTemplate.mock.calls[0];
        expect(call).toBeDefined();
        const [level, template, args] = call as [
            LogLevel,
            TemplateStringsArray,
            unknown[],
        ];
        expect(level).toBe("info");
        expect([...template]).toEqual(["just a static string"]);
        expect(args).toEqual([]);
    });
});

describe("AbstractLogger dispatch decisions", () => {
    it("treats a plain string with a sync function third arg as logLazy via log()", () => {
        const logger = new TestLogger();
        const compute = vi.fn(() => ({ a: 1 }));

        logger.log("info", "msg", compute);

        expect(logger.logLazy).toHaveBeenCalledTimes(1);
        expect(logger.logLazyAsync).not.toHaveBeenCalled();
        expect(compute).not.toHaveBeenCalled();
    });

    it("treats a plain string with an async function third arg as logLazyAsync via log()", async () => {
        const logger = new TestLogger();
        let invoked = false;
        const compute = async () => {
            invoked = true;
            return { a: 1 };
        };

        const result = logger.log("info", "msg", compute);

        expect(logger.logLazyAsync).toHaveBeenCalledTimes(1);
        expect(logger.logLazyAsync).toHaveBeenCalledWith(
            "info",
            "msg",
            compute,
        );
        expect(logger.logLazyAsync.mock.calls[0]?.[2]).toBe(compute);
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logEager).not.toHaveBeenCalled();
        expect(result).toBeInstanceOf(Promise);
        await expect(result).resolves.toBeUndefined();
        // The sink decides whether to invoke; the test default mock does not.
        expect(invoked).toBe(false);
    });

    it("treats a plain string with an object third arg as logEager via log()", () => {
        const logger = new TestLogger();
        const properties = { a: 1 };

        logger.log("info", "msg", properties);

        expect(logger.logEager).toHaveBeenCalledTimes(1);
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logLazyAsync).not.toHaveBeenCalled();
    });

    it("treats no message via log(level) as a deferred leveled function (no dispatch until invoked)", () => {
        const logger = new TestLogger();

        const leveled = logger.log("info");
        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logLazyAsync).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();

        leveled`x ${1}`;
        expect(logger.logTemplate).toHaveBeenCalledTimes(1);
    });
});

describe("AbstractLogger async deferred compute", () => {
    it("routes async compute to logLazyAsync via log() with the matching level", async () => {
        for (const level of LOG_LEVELS) {
            const logger = new TestLogger();
            let invoked = false;
            const compute = async () => {
                invoked = true;
                return { user: "alice", lvl: level };
            };

            const result = logger.log(level, `hello ${level}`, compute);

            expect(result).toBeInstanceOf(Promise);
            await expect(result).resolves.toBeUndefined();

            expect(logger.logLazyAsync).toHaveBeenCalledTimes(1);
            expect(logger.logLazyAsync).toHaveBeenCalledWith(
                level,
                `hello ${level}`,
                compute,
            );
            expect(logger.logLazyAsync.mock.calls[0]?.[2]).toBe(compute);
            expect(logger.logLazy).not.toHaveBeenCalled();
            expect(logger.logEager).not.toHaveBeenCalled();
            expect(logger.logTemplate).not.toHaveBeenCalled();
            expect(invoked).toBe(false);
        }
    });

    it("propagates the Promise returned by logLazyAsync to the caller of log()", async () => {
        const logger = new TestLogger();
        let resolveSink!: () => void;
        const sinkPromise = new Promise<void>((resolve) => {
            resolveSink = resolve;
        });
        logger.logLazyAsync.mockImplementation(() => sinkPromise);

        const compute = async () => ({ a: 1 });
        const result = logger.log("info", "msg", compute);

        expect(result).toBeInstanceOf(Promise);

        let settled = false;
        const tracked = (result as Promise<void>).then(() => {
            settled = true;
        });

        // Yield to the microtask queue: the result should still be pending.
        await Promise.resolve();
        expect(settled).toBe(false);

        resolveSink();
        await tracked;
        expect(settled).toBe(true);
    });

    it("propagates a rejection from logLazyAsync to the caller", async () => {
        const logger = new TestLogger();
        const failure = new Error("sink failed");
        logger.logLazyAsync.mockImplementation(() => Promise.reject(failure));

        const compute = async () => ({ a: 1 });
        const result = logger.log("info", "msg", compute);

        expect(result).toBeInstanceOf(Promise);
        await expect(result).rejects.toBe(failure);
    });

    it("routes async compute to logLazyAsync via leveled methods with the matching level", async () => {
        const LEVEL_TO_METHOD: Record<
            LogLevel,
            "error" | "warn" | "info" | "debug" | "trace"
        > = {
            error: "error",
            warn: "warn",
            info: "info",
            debug: "debug",
            trace: "trace",
        };

        for (const level of LOG_LEVELS) {
            const logger = new TestLogger();
            const method = LEVEL_TO_METHOD[level];
            let invoked = false;
            const compute = async () => {
                invoked = true;
                return { lvl: level };
            };

            const result = logger[method](`hello ${level}`, compute);

            expect(result).toBeInstanceOf(Promise);
            await expect(result).resolves.toBeUndefined();

            expect(logger.logLazyAsync).toHaveBeenCalledTimes(1);
            expect(logger.logLazyAsync).toHaveBeenCalledWith(
                level,
                `hello ${level}`,
                compute,
            );
            expect(logger.logLazy).not.toHaveBeenCalled();
            expect(logger.logEager).not.toHaveBeenCalled();
            expect(logger.logTemplate).not.toHaveBeenCalled();
            expect(invoked).toBe(false);
        }
    });

    it("routes async compute to logLazyAsync via the leveled function returned by log(level)", async () => {
        const logger = new TestLogger();
        const leveled = logger.log("debug");
        const compute = async () => ({ a: 1 });

        const result = leveled("compute me", compute);

        expect(result).toBeInstanceOf(Promise);
        await expect(result).resolves.toBeUndefined();

        expect(logger.logLazyAsync).toHaveBeenCalledTimes(1);
        expect(logger.logLazyAsync).toHaveBeenCalledWith(
            "debug",
            "compute me",
            compute,
        );
        expect(logger.logLazy).not.toHaveBeenCalled();
        expect(logger.logEager).not.toHaveBeenCalled();
        expect(logger.logTemplate).not.toHaveBeenCalled();
    });

    it("detects async arrow functions as async", async () => {
        const logger = new TestLogger();
        const arrow = async () => ({ a: 1 });

        const result = logger.log("info", "msg", arrow);

        expect(result).toBeInstanceOf(Promise);
        await result;
        expect(logger.logLazyAsync).toHaveBeenCalledTimes(1);
        expect(logger.logLazy).not.toHaveBeenCalled();
    });

    it("detects classic async function expressions as async", async () => {
        const logger = new TestLogger();
        const fn = async function compute() {
            return { a: 1 };
        };

        const result = logger.log("info", "msg", fn);

        expect(result).toBeInstanceOf(Promise);
        await result;
        expect(logger.logLazyAsync).toHaveBeenCalledTimes(1);
        expect(logger.logLazy).not.toHaveBeenCalled();
    });

    it("treats a plain function returning a Promise as synchronous (routes to logLazy)", () => {
        const logger = new TestLogger();
        // Not an `async` function: a regular function that happens to return a
        // Promise. This is documented to take the synchronous logLazy path.
        const compute = () => Promise.resolve({ a: 1 });

        const result = logger.log("info", "msg", compute);

        expect(result).toBeUndefined();
        expect(logger.logLazy).toHaveBeenCalledTimes(1);
        expect(logger.logLazyAsync).not.toHaveBeenCalled();
    });

    it("does not invoke the async compute (lazy semantics preserved)", async () => {
        const logger = new TestLogger();
        let invoked = false;
        const compute = async () => {
            invoked = true;
            return { a: 1 };
        };

        await logger.log("info", "msg", compute);

        // The default sink mock does not invoke the compute fn; this verifies
        // dispatch itself never invokes the user's async compute.
        expect(invoked).toBe(false);
    });
});
