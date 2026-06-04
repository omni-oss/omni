import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
    type ConsoleLike,
    createLogInterceptor,
    initLogInterceptor,
    LOG_LEVELS,
    type LogLevel,
} from ".";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

interface FakeConsole extends ConsoleLike {
    /** Captures everything that flows through to the original (post-patch) methods. */
    calls: { level: LogLevel; args: unknown[] }[];
}

function makeFakeConsole(): FakeConsole {
    const calls: { level: LogLevel; args: unknown[] }[] = [];
    const fake = { calls } as FakeConsole;
    for (const level of LOG_LEVELS) {
        fake[level] = (...args: unknown[]) => {
            calls.push({ level, args });
        };
    }
    return fake;
}

function delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

// ---------------------------------------------------------------------------
// initLogInterceptor
// ---------------------------------------------------------------------------

describe("initLogInterceptor", () => {
    it("captures entries for every log level with level/args/time", () => {
        const target = makeFakeConsole();
        let nowValue = 1_000;
        const logger = initLogInterceptor({
            target,
            clock: () => nowValue++,
        });

        for (const level of LOG_LEVELS) {
            target[level](`msg-${level}`, 42);
        }

        expect(logger.logs).toHaveLength(LOG_LEVELS.length);
        logger.logs.forEach((entry, i) => {
            expect(entry.level).toBe(LOG_LEVELS[i]);
            expect(entry.args).toEqual([`msg-${LOG_LEVELS[i]}`, 42]);
            expect(entry.time).toBe(1_000 + i);
        });
    });

    it("forwards to the original methods when passthrough=true", () => {
        const target = makeFakeConsole();
        initLogInterceptor({ target });

        target.log("hello");
        target.warn("oops");

        expect(target.calls).toEqual([
            { level: "log", args: ["hello"] },
            { level: "warn", args: ["oops"] },
        ]);
    });

    it("does NOT forward when passthrough=false", () => {
        const target = makeFakeConsole();
        const logger = initLogInterceptor({ target, passthrough: false });

        target.log("hidden");

        expect(target.calls).toEqual([]);
        expect(logger.logs).toHaveLength(1);
        expect(logger.logs[0]?.args).toEqual(["hidden"]);
    });

    it("respects max by dropping the oldest entries", () => {
        const target = makeFakeConsole();
        const logger = initLogInterceptor({
            target,
            max: 2,
            passthrough: false,
        });

        target.log("a");
        target.log("b");
        target.log("c");

        expect(logger.logs.map((e) => e.args[0])).toEqual(["b", "c"]);
    });

    it("snapshot returns a copy, not the live buffer", () => {
        const target = makeFakeConsole();
        const logger = initLogInterceptor({ target, passthrough: false });

        target.log("first");
        const snap = logger.snapshot();
        target.log("second");

        expect(snap).toHaveLength(1);
        expect(logger.logs).toHaveLength(2);
    });

    it("clear empties the buffer in place", () => {
        const target = makeFakeConsole();
        const logger = initLogInterceptor({ target, passthrough: false });
        const liveRef = logger.logs;

        target.log("a");
        target.log("b");
        logger.clear();

        expect(logger.logs).toBe(liveRef); // same reference
        expect(logger.logs).toHaveLength(0);
    });

    it("restore puts the original methods back", () => {
        const target = makeFakeConsole();
        const originals = LOG_LEVELS.map((l) => target[l]);

        const logger = initLogInterceptor({ target });
        // confirm methods were actually replaced
        for (const level of LOG_LEVELS) {
            expect(target[level]).not.toBe(
                originals[LOG_LEVELS.indexOf(level)],
            );
        }

        logger.restore();
        for (const level of LOG_LEVELS) {
            expect(target[level]).toBe(originals[LOG_LEVELS.indexOf(level)]);
        }
    });
});

// ---------------------------------------------------------------------------
// createLogInterceptor
// ---------------------------------------------------------------------------

describe("createLogInterceptor", () => {
    let target: FakeConsole;
    let interceptor: ReturnType<typeof createLogInterceptor>;

    beforeEach(() => {
        target = makeFakeConsole();
        interceptor = createLogInterceptor({ target, clock: () => 7 });
    });

    afterEach(() => {
        interceptor.uninstall();
    });

    it("captures every log level inside a scope", async () => {
        const { logs, result } = await interceptor.interceptLogs(() => {
            for (const level of LOG_LEVELS) {
                target[level](`hi-${level}`);
            }
            return "done";
        });

        expect(result).toBe("done");
        expect(logs.map((e) => e.level)).toEqual([...LOG_LEVELS]);
        expect(logs.every((e) => e.time === 7)).toBe(true);
    });

    it("auto-installs on first call and reports installed state", async () => {
        expect(interceptor.isInstalled()).toBe(false);
        await interceptor.interceptLogs(() => {});
        expect(interceptor.isInstalled()).toBe(true);
    });

    it("logs outside any scope still pass through and are not captured", async () => {
        interceptor.install();
        target.log("outside-before");

        const { logs } = await interceptor.interceptLogs(() => {
            target.log("inside");
        });

        target.log("outside-after");

        expect(logs.map((e) => e.args[0])).toEqual(["inside"]);
        expect(target.calls.map((c) => c.args[0])).toEqual([
            "outside-before",
            "inside",
            "outside-after",
        ]);
    });

    it("propagates passthrough=true by default", async () => {
        await interceptor.interceptLogs(() => {
            target.log("passes");
        });

        expect(target.calls).toEqual([{ level: "log", args: ["passes"] }]);
    });

    it("swallows logs when passthrough=false", async () => {
        const { logs } = await interceptor.interceptLogs(
            () => {
                target.log("hidden");
                target.error("boom");
            },
            { passthrough: false },
        );

        expect(logs.map((e) => e.args[0])).toEqual(["hidden", "boom"]);
        expect(target.calls).toEqual([]);
    });

    it("respects max by dropping oldest entries", async () => {
        const { logs } = await interceptor.interceptLogs(
            () => {
                target.log("a");
                target.log("b");
                target.log("c");
                target.log("d");
            },
            { max: 2, passthrough: false },
        );

        expect(logs.map((e) => e.args[0])).toEqual(["c", "d"]);
    });

    it("isolates concurrent scopes - each only sees its own logs", async () => {
        // Each function awaits across a few microtasks to interleave scheduling.
        const taskA = interceptor.interceptLogs(async () => {
            target.log("a1");
            await delay(5);
            target.log("a2");
            await delay(5);
            target.log("a3");
        });

        const taskB = interceptor.interceptLogs(async () => {
            target.log("b1");
            await delay(2);
            target.log("b2");
            await delay(8);
            target.log("b3");
        });

        const [a, b] = await Promise.all([taskA, taskB]);

        expect(a.logs.map((e) => e.args[0])).toEqual(["a1", "a2", "a3"]);
        expect(b.logs.map((e) => e.args[0])).toEqual(["b1", "b2", "b3"]);
    });

    it("nested scopes both receive inner logs (inner bubbles to outer)", async () => {
        const outer = await interceptor.interceptLogs(async () => {
            target.log("outer-before");

            const inner = await interceptor.interceptLogs(() => {
                target.log("inner");
            });

            target.log("outer-after");
            return inner;
        });

        expect(outer.logs.map((e) => e.args[0])).toEqual([
            "outer-before",
            "inner",
            "outer-after",
        ]);
        expect(outer.result.logs.map((e) => e.args[0])).toEqual(["inner"]);
    });

    it("inner passthrough=false suppresses passthrough even with passthrough outer", async () => {
        await interceptor.interceptLogs(async () => {
            target.log("outer-1");
            await interceptor.interceptLogs(
                () => {
                    target.log("inner-secret");
                },
                { passthrough: false },
            );
            target.log("outer-2");
        });

        // outer logs pass through, but the inner-secret one should not
        expect(target.calls.map((c) => c.args[0])).toEqual([
            "outer-1",
            "outer-2",
        ]);
    });

    it("uninstall restores original methods", async () => {
        const originals: Record<LogLevel, unknown> = {} as Record<
            LogLevel,
            unknown
        >;
        for (const level of LOG_LEVELS) originals[level] = target[level];

        interceptor.install();
        for (const level of LOG_LEVELS) {
            expect(target[level]).not.toBe(originals[level]);
        }

        interceptor.uninstall();
        for (const level of LOG_LEVELS) {
            expect(target[level]).toBe(originals[level]);
        }
        expect(interceptor.isInstalled()).toBe(false);
    });

    it("install and uninstall are idempotent", () => {
        interceptor.install();
        const patched = target.log;
        interceptor.install(); // second call should be a no-op
        expect(target.log).toBe(patched);

        interceptor.uninstall();
        interceptor.uninstall(); // should not throw
        expect(interceptor.isInstalled()).toBe(false);
    });

    it("uses the supplied clock for entry timestamps", async () => {
        const now = vi.fn<() => number>();
        now.mockReturnValueOnce(100)
            .mockReturnValueOnce(200)
            .mockReturnValueOnce(300);

        const localTarget = makeFakeConsole();
        const localInterceptor = createLogInterceptor({
            target: localTarget,
            clock: now,
        });

        const { logs } = await localInterceptor.interceptLogs(() => {
            localTarget.log("a");
            localTarget.log("b");
            localTarget.log("c");
        });

        localInterceptor.uninstall();

        expect(logs.map((e) => e.time)).toEqual([100, 200, 300]);
    });

    it("two independent interceptors targeting different consoles do not interfere", async () => {
        const targetA = makeFakeConsole();
        const targetB = makeFakeConsole();
        const intA = createLogInterceptor({ target: targetA, clock: () => 1 });
        const intB = createLogInterceptor({ target: targetB, clock: () => 2 });

        try {
            const [a, b] = await Promise.all([
                intA.interceptLogs(() => {
                    targetA.log("a");
                }),
                intB.interceptLogs(() => {
                    targetB.log("b");
                }),
            ]);

            expect(a.logs.map((e) => e.args[0])).toEqual(["a"]);
            expect(b.logs.map((e) => e.args[0])).toEqual(["b"]);
            expect(a.logs[0]?.time).toBe(1);
            expect(b.logs[0]?.time).toBe(2);
        } finally {
            intA.uninstall();
            intB.uninstall();
        }
    });
});
