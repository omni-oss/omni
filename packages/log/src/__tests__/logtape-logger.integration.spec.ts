import { configure, resetSync } from "@logtape/logtape";
import { afterAll, beforeEach, describe, expect, it } from "vitest";
import { LogTapeLoggerFactory } from "../logtape/logtape-logger";
import {
    type Capturer,
    createCapturer,
    factory,
    recordAt,
    setupCapture,
} from "./_helpers";

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

afterAll(() => {
    resetSync();
});

// ---------------------------------------------------------------------------
// LogTapeLoggerFactory + real logtape end-to-end
// ---------------------------------------------------------------------------

describe("LogTapeLoggerFactory + real logtape (end-to-end)", () => {
    let cap: Capturer;

    beforeEach(async () => {
        cap = createCapturer();
        await setupCapture(cap);
    });

    it("emits records to the underlying sink for every level", () => {
        const logger = factory.get(["app"]);

        logger.error("e");
        logger.warn("w");
        logger.info("i");
        logger.debug("d");
        logger.trace("t");

        expect(cap.records).toHaveLength(5);
        expect(cap.records.map((r) => r.level)).toEqual([
            "error",
            "warning", // 'warn' is translated to logtape's 'warning'
            "info",
            "debug",
            "trace",
        ]);
        expect(cap.records.map((r) => r.rawMessage)).toEqual([
            "e",
            "w",
            "i",
            "d",
            "t",
        ]);
        expect(cap.records.every((r) => r.category[0] === "app")).toBe(true);
    });

    it("substitutes named placeholders in messages with eager properties", () => {
        const logger = factory.get(["app"]);

        logger.info("hello {name}", { name: "alice" });

        expect(cap.records).toHaveLength(1);
        const record = recordAt(cap);
        expect(record.level).toBe("info");
        expect(record.rawMessage).toBe("hello {name}");
        expect(record.message).toEqual(["hello ", "alice", ""]);
        expect(record.properties).toEqual({ name: "alice" });
    });

    it("works with a string category passed to the factory", () => {
        const logger = factory.get("app");

        logger.info("hi");

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app"]);
    });

    it("works with a tuple category passed to the factory", async () => {
        await configure({
            reset: true,
            sinks: { capture: cap.sink },
            loggers: [
                {
                    category: ["app", "auth"],
                    sinks: ["capture"],
                    lowestLevel: "trace",
                },
                {
                    category: ["logtape", "meta"],
                    sinks: [],
                    lowestLevel: "warning",
                },
            ],
        });

        const logger = factory.get(["app", "auth"]);
        logger.info("hi");

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "auth"]);
    });

    it("the factory's loggers expose the logtape parent chain via the dynamic parent getter", () => {
        // The dynamic `parent` getter resolves to whatever logtape's own
        // Logger reports as its parent. For `["app"]`, that is logtape's
        // global root Logger (category `[]`), not `null` — so we walk the
        // chain and assert it terminates at `null` after one or more hops.
        const logger = factory.get(["app"]);

        let curr: typeof logger.parent = logger;
        let hops = 0;
        const HARD_LIMIT = 10;
        while (curr !== null && hops < HARD_LIMIT) {
            curr = curr.parent;
            hops += 1;
        }
        expect(curr).toBeNull();
        expect(hops).toBeGreaterThanOrEqual(1);
    });

    it("caches wrappers per category: repeated get() returns the same instance", () => {
        const a = factory.get(["app"]);
        const b = factory.get(["app"]);
        expect(a).toBe(b);
    });

    it("caches wrappers for different categories independently", () => {
        const x = factory.get(["app", "x"]);
        const y = factory.get(["app", "y"]);
        expect(x).not.toBe(y);
        // Each is still individually cached:
        expect(factory.get(["app", "x"])).toBe(x);
        expect(factory.get(["app", "y"])).toBe(y);
    });

    it("a fresh factory instance produces a different wrapper than the shared factory", () => {
        const fromShared = factory.get(["app"]);
        const isolatedFactory = new LogTapeLoggerFactory();
        const fromIsolated = isolatedFactory.get(["app"]);

        // The cache is keyed by a per-factory Symbol, so the two factories
        // produce distinct wrappers even though logtape's `getLogger`
        // returns the same underlying Logger for the same category.
        expect(fromIsolated).not.toBe(fromShared);
        // The new factory's wrapper is itself stable across calls:
        expect(isolatedFactory.get(["app"])).toBe(fromIsolated);
    });
});

// ---------------------------------------------------------------------------
// Lazy compute integration
// ---------------------------------------------------------------------------

describe("Lazy compute integration with real logtape", () => {
    let cap: Capturer;

    beforeEach(() => {
        cap = createCapturer();
    });

    it("invokes the sync compute fn when the level is enabled", async () => {
        await setupCapture(cap, "trace");
        const logger = factory.get(["app"]);
        let invocations = 0;
        const compute = () => {
            invocations += 1;
            return { user: "alice" };
        };

        logger.info("hello {user}", compute);

        expect(invocations).toBe(1);
        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).properties).toEqual({ user: "alice" });
    });

    it("does NOT invoke the sync compute fn when the level is filtered out", async () => {
        await setupCapture(cap, "info");
        const logger = factory.get(["app"]);
        let invocations = 0;
        const compute = () => {
            invocations += 1;
            return { expensive: true };
        };

        logger.debug("hello {expensive}", compute);
        logger.trace("hello {expensive}", compute);

        expect(invocations).toBe(0);
        expect(cap.records).toHaveLength(0);
    });

    it("invokes sync compute only for enabled levels in mixed calls", async () => {
        await setupCapture(cap, "info");
        const logger = factory.get(["app"]);
        let invocations = 0;
        const compute = () => {
            invocations += 1;
            return { v: 1 };
        };

        logger.debug("skip", compute); // filtered out
        logger.info("keep", compute); // emitted

        expect(invocations).toBe(1);
        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).level).toBe("info");
    });
});

// ---------------------------------------------------------------------------
// Async compute integration
// ---------------------------------------------------------------------------

describe("Async compute integration with real logtape", () => {
    let cap: Capturer;

    beforeEach(() => {
        cap = createCapturer();
    });

    it("awaits the async compute and includes the resolved properties", async () => {
        await setupCapture(cap, "trace");
        const logger = factory.get(["app"]);

        const result = logger.info("hello {user}", async () => ({
            user: "alice",
        }));

        expect(result).toBeInstanceOf(Promise);
        // Before awaiting, the sink may or may not have received the record yet
        // (logtape's contract: the returned promise resolves after the write).
        await result;

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).properties).toEqual({ user: "alice" });
        expect(recordAt(cap).message).toEqual(["hello ", "alice", ""]);
    });

    it("does NOT invoke the async compute when the level is filtered out", async () => {
        await setupCapture(cap, "info");
        const logger = factory.get(["app"]);
        let invocations = 0;

        const result = logger.debug("nope {x}", async () => {
            invocations += 1;
            return { x: 1 };
        });

        // Logtape returns a Promise even for skipped levels (or void); accept
        // either, but if a Promise is returned, awaiting it must resolve.
        if (result instanceof Promise) {
            await result;
        }

        expect(invocations).toBe(0);
        expect(cap.records).toHaveLength(0);
    });

    it("multiple sequential async logs are each awaited and ordered", async () => {
        await setupCapture(cap, "trace");
        const logger = factory.get(["app"]);

        await logger.info("first {n}", async () => ({ n: 1 }));
        await logger.info("second {n}", async () => ({ n: 2 }));

        expect(cap.records).toHaveLength(2);
        expect(recordAt(cap).properties).toEqual({ n: 1 });
        expect(recordAt(cap, 1).properties).toEqual({ n: 2 });
    });
});

// ---------------------------------------------------------------------------
// Tagged-template integration
// ---------------------------------------------------------------------------

describe("Tagged-template integration with real logtape", () => {
    let cap: Capturer;

    beforeEach(async () => {
        cap = createCapturer();
        await setupCapture(cap);
    });

    it("emits a record whose rawMessage is the template strings array", () => {
        const logger = factory.get(["app"]);

        const name = "world";
        logger.info`hello ${name}`;

        expect(cap.records).toHaveLength(1);
        const record = recordAt(cap);
        expect(record.level).toBe("info");
        // logtape preserves the template strings array as rawMessage when the
        // log is invoked as a tagged template.
        expect(Array.isArray(record.rawMessage)).toBe(true);
        expect([...(record.rawMessage as readonly string[])]).toEqual([
            "hello ",
            "",
        ]);
        expect(record.message).toEqual(["hello ", "world", ""]);
    });

    it("supports tagged templates with multiple interpolations", () => {
        const logger = factory.get(["app"]);

        const a = 1;
        const b = "two";
        logger.error`a=${a} b=${b}`;

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).level).toBe("error");
        expect(recordAt(cap).message).toEqual(["a=", 1, " b=", "two", ""]);
    });

    it("routes warn-tagged-templates to logtape's 'warning' level", () => {
        const logger = factory.get(["app"]);

        logger.warn`careful ${42}`;

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).level).toBe("warning");
    });
});

// ---------------------------------------------------------------------------
// enabled() integration
// ---------------------------------------------------------------------------

describe("enabled() integration with real logtape", () => {
    let cap: Capturer;

    beforeEach(() => {
        cap = createCapturer();
    });

    it("reflects the configured lowestLevel for each level", async () => {
        await setupCapture(cap, "info");
        const logger = factory.get(["app"]);

        expect(logger.enabled("error")).toBe(true);
        expect(logger.enabled("warn")).toBe(true);
        expect(logger.enabled("info")).toBe(true);
        expect(logger.enabled("debug")).toBe(false);
        expect(logger.enabled("trace")).toBe(false);
    });

    it("returns true for everything when lowestLevel is 'trace'", async () => {
        await setupCapture(cap, "trace");
        const logger = factory.get(["app"]);

        for (const level of [
            "error",
            "warn",
            "info",
            "debug",
            "trace",
        ] as const) {
            expect(logger.enabled(level)).toBe(true);
        }
    });
});

// ---------------------------------------------------------------------------
// child() integration
// ---------------------------------------------------------------------------

describe("child() integration with real logtape", () => {
    let cap: Capturer;

    beforeEach(async () => {
        cap = createCapturer();
        await setupCapture(cap);
    });

    it("logs from child loggers carry the extended category", () => {
        const root = factory.get(["app"]);
        const child = root.child("auth");

        child.info("hi");

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "auth"]);
    });

    it("supports tuple subcategories", () => {
        const root = factory.get(["app"]);
        const child = root.child(["auth", "session"]);

        child.info("hi");

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "auth", "session"]);
    });

    it("the child's parent field references the original wrapper", () => {
        // The dynamic `parent` getter walks logtape's parent chain and
        // resolves through the factory's cache, so the parent of the
        // child wrapper is the same wrapper instance as `root`, regardless
        // of which access path populated the cache slot first.
        const root = factory.get(["app"]);
        const child = root.child("auth");

        expect(child.parent).toBe(root);
    });

    it("deep child chains preserve the parent linkage", () => {
        const root = factory.get(["app"]);
        const a = root.child("a");
        const b = a.child("b");
        const c = b.child("c");

        expect(c.parent).toBe(b);
        expect(b.parent).toBe(a);
        expect(a.parent).toBe(root);
        // `root.parent` is no longer `null` — the dynamic getter resolves
        // to a wrapper around logtape's global root logger. Verify the
        // chain still terminates at `null` after walking up.
        let curr: typeof root.parent = root.parent;
        let hops = 0;
        while (curr !== null && hops < 10) {
            curr = curr.parent;
            hops += 1;
        }
        expect(curr).toBeNull();

        c.info("deep");
        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "a", "b", "c"]);
    });

    it("inherits sinks from the configured ancestor by default", () => {
        // Only ["app"] is configured with a sink. A grandchild category is
        // never explicitly configured, but by logtape's default
        // parentSinks: 'inherit', it should still be captured.
        const grandchild = factory.get(["app"]).child("a").child("b");

        grandchild.info("hi");

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "a", "b"]);
    });

    it("caches child wrappers per subcategory: repeated child() returns the same instance", () => {
        const root = factory.get(["app"]);
        const a = root.child("auth");
        const b = root.child("auth");
        expect(a).toBe(b);
    });

    it("caches child wrappers for different subcategories independently", () => {
        const root = factory.get(["app"]);
        const x = root.child("x");
        const y = root.child("y");
        expect(x).not.toBe(y);
        // Each is still individually cache-stable:
        expect(root.child("x")).toBe(x);
        expect(root.child("y")).toBe(y);
    });

    it("the cached child preserves its parent reference", () => {
        // The dynamic `parent` getter resolves through the factory's
        // unified cache, so the parent of the cached child is `root`
        // regardless of which access path populated the slot first.
        const root = factory.get(["app"]);
        const child = root.child("auth");

        expect(child.parent).toBe(root);
        // Cache lookup still returns a wrapper that points back at root.
        expect(root.child("auth").parent).toBe(root);
    });

    it("deep child chains are also cached", () => {
        const root = factory.get(["app"]);
        const a = root.child("a");
        const b1 = a.child("b");
        const b2 = a.child("b");

        expect(b1).toBe(b2);
        // The full chain is composed of stable wrappers, end-to-end.
        expect(root.child("a").child("b")).toBe(b1);
    });

    it("two distinct factories produce distinct cached child wrappers (even for the same subcategory)", () => {
        const root1 = factory.get(["app"]);
        const isolatedFactory = new LogTapeLoggerFactory();
        const root2 = isolatedFactory.get(["app"]);

        const child1 = root1.child("auth");
        const child2 = root2.child("auth");

        expect(child1).not.toBe(child2);
        // Each is still cached against its own parent:
        expect(root1.child("auth")).toBe(child1);
        expect(root2.child("auth")).toBe(child2);
    });

    it("factory.get([root, sub]) and factory.get([root]).child(sub) converge on the same wrapper (unified cache key)", () => {
        // The factory's Symbol is threaded through every wrapper it
        // produces, so both access paths attach to the *same* slot on the
        // underlying logtape Logger and cache hits cross routes.
        const isolatedFactory = new LogTapeLoggerFactory();
        const viaDirect = isolatedFactory.get(["app", "converge-direct-first"]);
        const viaChain = isolatedFactory
            .get(["app"])
            .child("converge-direct-first");
        expect(viaChain).toBe(viaDirect);

        // And the reverse access order also unifies:
        const viaChainFirst = isolatedFactory
            .get(["app"])
            .child("converge-chain-first");
        const viaDirectAfter = isolatedFactory.get([
            "app",
            "converge-chain-first",
        ]);
        expect(viaDirectAfter).toBe(viaChainFirst);
    });
});

// ---------------------------------------------------------------------------
// with() integration
// ---------------------------------------------------------------------------

describe("with() integration with real logtape", () => {
    let cap: Capturer;

    beforeEach(async () => {
        cap = createCapturer();
        await setupCapture(cap);
    });

    it("merges contextual properties into emitted records", () => {
        const root = factory.get(["app"]);
        const ctx = root.with({ requestId: "req-1" });

        ctx.info("hello {name}", { name: "alice" });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).properties).toEqual({
            requestId: "req-1",
            name: "alice",
        });
    });

    it("does not affect the original logger", () => {
        const root = factory.get(["app"]);
        const ctx = root.with({ requestId: "req-1" });

        ctx.info("from ctx");
        root.info("from root");

        expect(cap.records).toHaveLength(2);
        expect(recordAt(cap).properties).toEqual({ requestId: "req-1" });
        expect(recordAt(cap, 1).properties).toEqual({});
    });

    it("supports chaining with().with() and merges all properties", () => {
        const root = factory.get(["app"]);
        const a = root.with({ a: 1 });
        const ab = a.with({ b: 2 });

        ab.info("hi");

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).properties).toEqual({ a: 1, b: 2 });
    });

    it("contextual logger preserves the category of its source", () => {
        const child = factory.get(["app"]).child("auth");
        const ctx = child.with({ requestId: "req-1" });

        ctx.info("hi");

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "auth"]);
        expect(recordAt(cap).properties).toEqual({ requestId: "req-1" });
    });
});

// ---------------------------------------------------------------------------
// Hierarchical config integration
// ---------------------------------------------------------------------------

describe("Hierarchical config integration", () => {
    let cap: Capturer;

    beforeEach(() => {
        cap = createCapturer();
    });

    it("applies category-specific lowestLevel overrides", async () => {
        await configure({
            reset: true,
            sinks: { capture: cap.sink },
            loggers: [
                { category: ["app"], sinks: ["capture"], lowestLevel: "info" },
                {
                    category: ["app", "verbose"],
                    sinks: ["capture"],
                    lowestLevel: "trace",
                },
                {
                    category: ["logtape", "meta"],
                    sinks: [],
                    lowestLevel: "warning",
                },
            ],
        });

        const main = factory.get(["app"]);
        const verbose = factory.get(["app", "verbose"]);

        main.debug("skipped"); // filtered: app's lowestLevel is info
        verbose.debug("kept"); // app.verbose allows debug

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "verbose"]);
        expect(recordAt(cap).message).toEqual(["kept"]);
    });

    it("respects parentSinks: 'override' (child does not inherit parent sinks)", async () => {
        const childCap = createCapturer();
        await configure({
            reset: true,
            sinks: { rootCap: cap.sink, childCap: childCap.sink },
            loggers: [
                { category: ["app"], sinks: ["rootCap"], lowestLevel: "trace" },
                {
                    category: ["app", "isolated"],
                    sinks: ["childCap"],
                    parentSinks: "override",
                    lowestLevel: "trace",
                },
                {
                    category: ["logtape", "meta"],
                    sinks: [],
                    lowestLevel: "warning",
                },
            ],
        });

        const isolated = factory.get(["app", "isolated"]);
        isolated.info("solo");

        expect(childCap.records).toHaveLength(1);
        expect(cap.records).toHaveLength(0);
    });
});
