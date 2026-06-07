import { LOG_LEVELS, type LogLevel } from "@omni-oss/log";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
    adaptConsole,
    type ConsoleLike,
    type ConsoleMethod,
    type ConsoleSource,
    createLogInterceptor,
    initLogInterceptor,
    type LogEntry,
} from "./interceptor";

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

interface FakeSource extends ConsoleSource {
    /** Captures every method invocation, including `log`. */
    calls: { method: keyof ConsoleSource; args: unknown[] }[];
}

function makeFakeSource(): FakeSource {
    const calls: { method: keyof ConsoleSource; args: unknown[] }[] = [];
    const source = { calls } as FakeSource;
    for (const method of [
        "log",
        "info",
        "warn",
        "error",
        "debug",
        "trace",
    ] as const) {
        source[method] = (...args: unknown[]) => {
            calls.push({ method, args });
        };
    }
    return source;
}

function delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

// ---------------------------------------------------------------------------
// LOG_LEVELS
// ---------------------------------------------------------------------------

describe("LOG_LEVELS", () => {
    it("contains exactly the supported levels", () => {
        expect([...LOG_LEVELS]).toEqual([
            "error",
            "warn",
            "info",
            "debug",
            "trace",
        ]);
    });

    it("does not include 'log'", () => {
        expect((LOG_LEVELS as readonly string[]).includes("log")).toBe(false);
    });
});

// ---------------------------------------------------------------------------
// adaptConsole
// ---------------------------------------------------------------------------

describe("adaptConsole", () => {
    it("exposes exactly the supported log levels", () => {
        const source = makeFakeSource();
        const adapter = adaptConsole(source);

        for (const level of LOG_LEVELS) {
            expect(typeof adapter[level]).toBe("function");
        }
        // No `log` on the adapter shape.
        expect("log" in adapter).toBe(false);
    });

    it("delegates reads to the underlying source", () => {
        const source = makeFakeSource();
        const adapter = adaptConsole(source);

        expect(adapter.error).toBe(source.error);
        expect(adapter.warn).toBe(source.warn);
        expect(adapter.info).toBe(source.info);
        expect(adapter.debug).toBe(source.debug);
        expect(adapter.trace).toBe(source.trace);
    });

    it("writes to a single method on the source for non-info levels", () => {
        const source = makeFakeSource();
        const adapter = adaptConsole(source);

        const before = {
            log: source.log,
            info: source.info,
            warn: source.warn,
            error: source.error,
            debug: source.debug,
            trace: source.trace,
        };

        const fn: ConsoleMethod = () => {};
        adapter.error = fn;
        expect(source.error).toBe(fn);
        // No other method was touched.
        expect(source.log).toBe(before.log);
        expect(source.info).toBe(before.info);
        expect(source.warn).toBe(before.warn);
        expect(source.debug).toBe(before.debug);
        expect(source.trace).toBe(before.trace);
    });

    it("writing `info` also overwrites `log` on the source", () => {
        const source = makeFakeSource();
        const adapter = adaptConsole(source);

        const fn: ConsoleMethod = vi.fn();
        adapter.info = fn;

        expect(source.info).toBe(fn);
        expect(source.log).toBe(fn);
    });

    it("captures `source.log(...)` as level `info` once installed", () => {
        const source = makeFakeSource();
        const adapter = adaptConsole(source);
        const logger = initLogInterceptor({
            target: adapter,
            passthrough: false,
        });

        source.log("via-log", 1);
        source.info("via-info", 2);

        expect(logger.logs.map((e) => [e.level, e.args])).toEqual([
            ["info", ["via-log", 1]],
            ["info", ["via-info", 2]],
        ]);

        logger.restore();
    });

    it("defaults to the global console when no source is provided", () => {
        // We don't want to actually patch the real console here, but we can
        // at least verify that `adaptConsole()` returns an object whose
        // accessors point at the global `console`.
        const adapter = adaptConsole();
        expect(adapter.error).toBe(console.error);
        expect(adapter.warn).toBe(console.warn);
        expect(adapter.info).toBe(console.info);
        expect(adapter.debug).toBe(console.debug);
        expect(adapter.trace).toBe(console.trace);
    });
});

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

        target.info("hello");
        target.warn("oops");

        expect(target.calls).toEqual([
            { level: "info", args: ["hello"] },
            { level: "warn", args: ["oops"] },
        ]);
    });

    it("does NOT forward when passthrough=false", () => {
        const target = makeFakeConsole();
        const logger = initLogInterceptor({ target, passthrough: false });

        target.info("hidden");

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

        target.info("a");
        target.info("b");
        target.info("c");

        expect(logger.logs.map((e) => e.args[0])).toEqual(["b", "c"]);
    });

    it("snapshot returns a copy, not the live buffer", () => {
        const target = makeFakeConsole();
        const logger = initLogInterceptor({ target, passthrough: false });

        target.info("first");
        const snap = logger.snapshot();
        target.info("second");

        expect(snap).toHaveLength(1);
        expect(logger.logs).toHaveLength(2);
    });

    it("clear empties the buffer in place", () => {
        const target = makeFakeConsole();
        const logger = initLogInterceptor({ target, passthrough: false });
        const liveRef = logger.logs;

        target.info("a");
        target.info("b");
        logger.clear();

        expect(logger.logs).toBe(liveRef); // same reference
        expect(logger.logs).toHaveLength(0);
    });

    it("invokes listeners passed via options for each entry, before passthrough", () => {
        const target = makeFakeConsole();
        const seen: LogEntry[] = [];
        const listener = vi.fn((entry: LogEntry) => {
            // At the moment the listener fires, the original method should not
            // have been called yet for this entry.
            expect(target.calls).toHaveLength(seen.length);
            seen.push(entry);
        });

        initLogInterceptor({ target, listeners: [listener] });

        target.info("first", 1);
        target.warn("second");

        expect(listener).toHaveBeenCalledTimes(2);
        expect(seen.map((e) => [e.level, e.args])).toEqual([
            ["info", ["first", 1]],
            ["warn", ["second"]],
        ]);
        // After both calls, both have been forwarded to the original methods.
        expect(target.calls.map((c) => c.args[0])).toEqual(["first", "second"]);
    });

    it("invokes multiple listeners in registration order", () => {
        const target = makeFakeConsole();
        const order: string[] = [];
        initLogInterceptor({
            target,
            passthrough: false,
            listeners: [
                () => order.push("a"),
                () => order.push("b"),
                () => order.push("c"),
            ],
        });

        target.info("x");

        expect(order).toEqual(["a", "b", "c"]);
    });

    it("addListener / removeListener manage subscribers at runtime", () => {
        const target = makeFakeConsole();
        const logger = initLogInterceptor({ target, passthrough: false });
        const calls: string[] = [];
        const listener = (entry: LogEntry) => calls.push(String(entry.args[0]));

        logger.addListener(listener);
        target.info("one");
        target.info("two");

        expect(calls).toEqual(["one", "two"]);

        const removed = logger.removeListener(listener);
        expect(removed).toBe(true);

        target.info("three");
        expect(calls).toEqual(["one", "two"]);

        // Removing again returns false.
        expect(logger.removeListener(listener)).toBe(false);
    });

    it("reflects mutations to the supplied listeners array on subsequent calls", () => {
        const target = makeFakeConsole();
        const seen: string[] = [];
        const listeners = [
            (entry: LogEntry) => seen.push(`a:${entry.args[0]}`),
        ];

        initLogInterceptor({ target, passthrough: false, listeners });

        target.info("1");
        listeners.push((entry) => seen.push(`b:${entry.args[0]}`));
        target.info("2");

        expect(seen).toEqual(["a:1", "a:2", "b:2"]);
    });

    it("isolates listener exceptions so logging keeps working", () => {
        const target = makeFakeConsole();
        const good = vi.fn();
        const bad = vi.fn(() => {
            throw new Error("listener boom");
        });

        initLogInterceptor({
            target,
            passthrough: false,
            listeners: [bad, good],
        });

        // The throwing listener triggers an error report through the captured
        // original `console.error`, which lands in `target.calls`.
        expect(() => target.info("hi")).not.toThrow();

        expect(bad).toHaveBeenCalledTimes(1);
        expect(good).toHaveBeenCalledTimes(1);
        const reported = target.calls.find(
            (c) => c.level === "error" && c.args[0] === "log listener threw:",
        );
        expect(reported).toBeDefined();
    });

    it("captures `source.log(...)` as level info when wrapping an adapter", () => {
        const source = makeFakeSource();
        const logger = initLogInterceptor({ target: adaptConsole(source) });

        source.log("via-log");
        source.info("via-info");
        source.warn("via-warn");

        expect(logger.logs.map((e) => [e.level, e.args[0]])).toEqual([
            ["info", "via-log"],
            ["info", "via-info"],
            ["warn", "via-warn"],
        ]);

        logger.restore();
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
        target.info("outside-before");

        const { logs } = await interceptor.interceptLogs(() => {
            target.info("inside");
        });

        target.info("outside-after");

        expect(logs.map((e) => e.args[0])).toEqual(["inside"]);
        expect(target.calls.map((c) => c.args[0])).toEqual([
            "outside-before",
            "inside",
            "outside-after",
        ]);
    });

    it("propagates passthrough=true by default", async () => {
        await interceptor.interceptLogs(() => {
            target.info("passes");
        });

        expect(target.calls).toEqual([{ level: "info", args: ["passes"] }]);
    });

    it("swallows logs when passthrough=false", async () => {
        const { logs } = await interceptor.interceptLogs(
            () => {
                target.info("hidden");
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
                target.info("a");
                target.info("b");
                target.info("c");
                target.info("d");
            },
            { max: 2, passthrough: false },
        );

        expect(logs.map((e) => e.args[0])).toEqual(["c", "d"]);
    });

    it("isolates concurrent scopes - each only sees its own logs", async () => {
        // Each function awaits across a few microtasks to interleave scheduling.
        const taskA = interceptor.interceptLogs(async () => {
            target.info("a1");
            await delay(5);
            target.info("a2");
            await delay(5);
            target.info("a3");
        });

        const taskB = interceptor.interceptLogs(async () => {
            target.info("b1");
            await delay(2);
            target.info("b2");
            await delay(8);
            target.info("b3");
        });

        const [a, b] = await Promise.all([taskA, taskB]);

        expect(a.logs.map((e) => e.args[0])).toEqual(["a1", "a2", "a3"]);
        expect(b.logs.map((e) => e.args[0])).toEqual(["b1", "b2", "b3"]);
    });

    it("nested scopes both receive inner logs (inner bubbles to outer)", async () => {
        const outer = await interceptor.interceptLogs(async () => {
            target.info("outer-before");

            const inner = await interceptor.interceptLogs(() => {
                target.info("inner");
            });

            target.info("outer-after");
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
            target.info("outer-1");
            await interceptor.interceptLogs(
                () => {
                    target.info("inner-secret");
                },
                { passthrough: false },
            );
            target.info("outer-2");
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
        const patched = target.info;
        interceptor.install(); // second call should be a no-op
        expect(target.info).toBe(patched);

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
            localTarget.info("a");
            localTarget.info("b");
            localTarget.info("c");
        });

        localInterceptor.uninstall();

        expect(logs.map((e) => e.time)).toEqual([100, 200, 300]);
    });

    it("invokes scope listeners with each entry inside that scope", async () => {
        const seen: LogEntry[] = [];
        const listener = vi.fn((entry: LogEntry) => seen.push(entry));

        const { logs } = await interceptor.interceptLogs(
            () => {
                target.info("a");
                target.error("b");
            },
            { listeners: [listener] },
        );

        expect(listener).toHaveBeenCalledTimes(2);
        expect(seen.map((e) => [e.level, e.args[0]])).toEqual([
            ["info", "a"],
            ["error", "b"],
        ]);
        expect(seen).toEqual(logs);
    });

    it("scope listeners only receive entries from their own scope", async () => {
        const aSeen: string[] = [];
        const bSeen: string[] = [];

        const taskA = interceptor.interceptLogs(
            async () => {
                target.info("a1");
                await delay(3);
                target.info("a2");
            },
            {
                listeners: [(e) => aSeen.push(String(e.args[0]))],
            },
        );

        const taskB = interceptor.interceptLogs(
            async () => {
                target.info("b1");
                await delay(1);
                target.info("b2");
            },
            {
                listeners: [(e) => bSeen.push(String(e.args[0]))],
            },
        );

        await Promise.all([taskA, taskB]);

        expect(aSeen).toEqual(["a1", "a2"]);
        expect(bSeen).toEqual(["b1", "b2"]);
    });

    it("nested scopes each fire their own listeners (inner bubbles to outer)", async () => {
        const outerSeen: string[] = [];
        const innerSeen: string[] = [];

        await interceptor.interceptLogs(
            async () => {
                target.info("outer-1");
                await interceptor.interceptLogs(
                    () => {
                        target.info("inner");
                    },
                    { listeners: [(e) => innerSeen.push(String(e.args[0]))] },
                );
                target.info("outer-2");
            },
            { listeners: [(e) => outerSeen.push(String(e.args[0]))] },
        );

        expect(outerSeen).toEqual(["outer-1", "inner", "outer-2"]);
        expect(innerSeen).toEqual(["inner"]);
    });

    it("global listeners (constructor option) fire for entries inside and outside scopes", async () => {
        const localTarget = makeFakeConsole();
        const seen: { level: LogLevel; arg: unknown }[] = [];
        const listener = (entry: LogEntry) =>
            seen.push({ level: entry.level, arg: entry.args[0] });

        const local = createLogInterceptor({
            target: localTarget,
            listeners: [listener],
        });

        try {
            local.install();
            localTarget.info("outside-before");

            await local.interceptLogs(() => {
                localTarget.info("inside");
            });

            localTarget.warn("outside-after");
        } finally {
            local.uninstall();
        }

        expect(seen).toEqual([
            { level: "info", arg: "outside-before" },
            { level: "info", arg: "inside" },
            { level: "warn", arg: "outside-after" },
        ]);
    });

    it("addListener returns a disposer that removes the listener", async () => {
        const seen: string[] = [];
        const listener = (entry: LogEntry) => seen.push(String(entry.args[0]));

        const dispose = interceptor.addListener(listener);
        interceptor.install();

        target.info("first");
        dispose();
        target.info("second");

        expect(seen).toEqual(["first"]);
        // Calling the disposer again is harmless.
        expect(() => dispose()).not.toThrow();
    });

    it("removeListener returns true when removed and false when not present", () => {
        const listener = () => {};
        expect(interceptor.removeListener(listener)).toBe(false);
        interceptor.addListener(listener);
        expect(interceptor.removeListener(listener)).toBe(true);
        expect(interceptor.removeListener(listener)).toBe(false);
    });

    it("global and scope listeners both fire for entries within a scope", async () => {
        const globalSeen: string[] = [];
        const scopeSeen: string[] = [];

        interceptor.addListener((e) => globalSeen.push(String(e.args[0])));

        await interceptor.interceptLogs(
            () => {
                target.info("x");
                target.info("y");
            },
            { listeners: [(e) => scopeSeen.push(String(e.args[0]))] },
        );

        expect(scopeSeen).toEqual(["x", "y"]);
        expect(globalSeen).toEqual(["x", "y"]);
    });

    it("isolates listener exceptions so the patched console keeps working", async () => {
        const good = vi.fn();
        interceptor.addListener(() => {
            throw new Error("global listener boom");
        });

        const { logs } = await interceptor.interceptLogs(
            () => {
                target.info("hello");
            },
            {
                passthrough: false,
                listeners: [
                    () => {
                        throw new Error("scope listener boom");
                    },
                    good,
                ],
            },
        );

        // The well-behaved listener still ran despite a sibling throwing.
        expect(good).toHaveBeenCalledTimes(1);
        // The entry was still captured in the scope buffer.
        expect(logs.map((e) => e.args[0])).toEqual(["hello"]);
        // Errors were reported via the original console.error, which our fake
        // captures into `target.calls`.
        const errorReports = target.calls.filter(
            (c) => c.level === "error" && c.args[0] === "log listener threw:",
        );
        expect(errorReports.length).toBeGreaterThanOrEqual(2);
    });

    it("captures `source.log(...)` as level info when wrapping an adapter", async () => {
        const source = makeFakeSource();
        const local = createLogInterceptor({ target: adaptConsole(source) });

        const { logs } = await local.interceptLogs(
            () => {
                source.log("via-log", 1);
                source.info("via-info", 2);
                source.warn("via-warn");
            },
            { passthrough: false },
        );

        local.uninstall();

        expect(logs.map((e) => [e.level, e.args])).toEqual([
            ["info", ["via-log", 1]],
            ["info", ["via-info", 2]],
            ["warn", ["via-warn"]],
        ]);
    });

    it("two independent interceptors targeting different consoles do not interfere", async () => {
        const targetA = makeFakeConsole();
        const targetB = makeFakeConsole();
        const intA = createLogInterceptor({ target: targetA, clock: () => 1 });
        const intB = createLogInterceptor({ target: targetB, clock: () => 2 });

        try {
            const [a, b] = await Promise.all([
                intA.interceptLogs(() => {
                    targetA.info("a");
                }),
                intB.interceptLogs(() => {
                    targetB.info("b");
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
