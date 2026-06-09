import { resetSync } from "@logtape/logtape";
import { afterAll, beforeEach, describe, expect, it } from "vitest";
import { ambientContextKind, createAmbientContext } from "../ambient-context";
import type { Logger } from "../core";
import { Log } from "../log";
import { withLogTapeRoot, withLogTapeRootSync } from "../logtape";
import {
    type Capturer,
    captureConfig,
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
// AmbientContext platform selection
// ---------------------------------------------------------------------------

describe("AmbientContext platform selection (in this Node test runtime)", () => {
    it("selects the AsyncLocalStorage backing implementation under Node", () => {
        // Vitest runs on Node, so we must end up on the real ALS.
        expect(ambientContextKind()).toBe("async-local-storage");
    });

    it("createAmbientContext returns independent instances", () => {
        const a = createAmbientContext<string>();
        const b = createAmbientContext<string>();

        a.run("alpha", () => {
            expect(a.getStore()).toBe("alpha");
            // Sibling context is unaffected.
            expect(b.getStore()).toBeUndefined();
        });
    });

    it("propagates the ambient value across awaits (ALS contract)", async () => {
        const ctx = createAmbientContext<number>();

        await ctx.run(7, async () => {
            await new Promise((r) => setTimeout(r, 0));
            expect(ctx.getStore()).toBe(7);
        });

        expect(ctx.getStore()).toBeUndefined();
    });

    it("forwards the callback's sync return value", () => {
        const ctx = createAmbientContext<string>();
        const result = ctx.run("anything", () => 42);
        expect(result).toBe(42);
    });

    it("forwards the callback's async return value as a Promise", async () => {
        const ctx = createAmbientContext<string>();
        const result = ctx.run("anything", async () => 99);
        expect(result).toBeInstanceOf(Promise);
        expect(await result).toBe(99);
    });

    it("supports nested run() calls (innermost wins)", () => {
        const ctx = createAmbientContext<string>();
        ctx.run("outer", () => {
            expect(ctx.getStore()).toBe("outer");
            ctx.run("inner", () => {
                expect(ctx.getStore()).toBe("inner");
            });
            expect(ctx.getStore()).toBe("outer");
        });
    });

    it("removes the ambient value when the callback throws", () => {
        const ctx = createAmbientContext<string>();
        expect(() =>
            ctx.run("transient", () => {
                throw new Error("boom");
            }),
        ).toThrow("boom");
        expect(ctx.getStore()).toBeUndefined();
    });

    it("removes the ambient value when an async callback rejects", async () => {
        const ctx = createAmbientContext<string>();
        await expect(
            ctx.run("transient", async () => {
                throw new Error("boom");
            }),
        ).rejects.toThrow("boom");
        expect(ctx.getStore()).toBeUndefined();
    });
});

// ---------------------------------------------------------------------------
// Log.withRoot
// ---------------------------------------------------------------------------

describe("Log.withRoot", () => {
    let cap: Capturer;

    beforeEach(async () => {
        cap = createCapturer();
        await setupCapture(cap);
    });

    it("creates the root logger via factory.get(category) and scopes it", () => {
        Log.withRoot(factory, ["app"], () => {
            expect(Log.isInitialized()).toBe(true);
            Log.info("hello");
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app"]);
    });

    it("supports a string category", () => {
        Log.withRoot(factory, "app", () => {
            Log.info("hi");
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app"]);
    });

    it("supports a tuple category", () => {
        Log.withRoot(factory, ["app"], () => {
            // category ["app"] is configured in the test capturer.
            Log.info("hi");
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app"]);
    });

    it("returns the callback's sync value", () => {
        const result = Log.withRoot(factory, ["app"], () => 42);
        expect(result).toBe(42);
    });

    it("forwards the callback's async return", async () => {
        const result = await Log.withRoot(factory, ["app"], async () => {
            Log.info("inside async");
            return 99;
        });

        expect(result).toBe(99);
        expect(cap.records).toHaveLength(1);
    });

    it("ambient is restored after the callback returns (sync)", () => {
        Log.withRoot(factory, ["app"], () => {
            expect(Log.isInitialized()).toBe(true);
        });
        expect(Log.isInitialized()).toBe(false);
    });

    it("ambient is restored after the callback rejects", async () => {
        await expect(
            Log.withRoot(factory, ["app"], async () => {
                throw new Error("boom");
            }),
        ).rejects.toThrow("boom");
        expect(Log.isInitialized()).toBe(false);
    });

    it("can be re-entered after the previous root completes", () => {
        Log.withRoot(factory, ["app"], () => {
            Log.info("first");
        });

        // Outside the root again — re-entry must succeed.
        Log.withRoot(factory, ["app"], () => {
            Log.info("second");
        });

        expect(cap.records).toHaveLength(2);
        expect(recordAt(cap).message).toEqual(["first"]);
        expect(recordAt(cap, 1).message).toEqual(["second"]);
    });

    it("throws when nested inside another active withRoot", () => {
        Log.withRoot(factory, ["app"], () => {
            expect(() =>
                Log.withRoot(factory, ["app"], () => {
                    /* unreachable */
                }),
            ).toThrow(/single root/i);
        });
    });

    it("throws when nested inside an async withRoot", async () => {
        await Log.withRoot(factory, ["app"], async () => {
            await new Promise((r) => setTimeout(r, 0));
            expect(() =>
                Log.withRoot(factory, ["app"], () => {
                    /* unreachable */
                }),
            ).toThrow(/single root/i);
        });
    });
});

// ---------------------------------------------------------------------------
// Log.withChild
// ---------------------------------------------------------------------------

describe("Log.withChild", () => {
    let cap: Capturer;

    beforeEach(async () => {
        cap = createCapturer();
        await setupCapture(cap);
    });

    it("creates a child of the ambient logger and scopes it", () => {
        Log.withRoot(factory, ["app"], () => {
            Log.withChild("auth", () => {
                Log.info("hi");
            });
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "auth"]);
    });

    it("supports tuple subcategories", () => {
        Log.withRoot(factory, ["app"], () => {
            Log.withChild(["auth", "session"], () => {
                Log.info("hi");
            });
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "auth", "session"]);
    });

    it("can be nested arbitrarily deep", () => {
        Log.withRoot(factory, ["app"], () => {
            Log.withChild("a", () => {
                Log.withChild("b", () => {
                    Log.withChild("c", () => {
                        Log.info("deep");
                    });
                });
            });
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "a", "b", "c"]);
    });

    it("restores the parent ambient logger after the child scope exits", () => {
        Log.withRoot(factory, ["app"], () => {
            const root = Log.instance();

            Log.withChild("auth", () => {
                expect(Log.instance()).not.toBe(root);
                Log.info("from child");
            });

            // After the child scope exits, the ambient logger is the same
            // root reference we captured before the child.
            expect(Log.instance()).toBe(root);
            Log.info("from root");
        });

        expect(cap.records).toHaveLength(2);
        expect(recordAt(cap).category).toEqual(["app", "auth"]);
        expect(recordAt(cap, 1).category).toEqual(["app"]);
    });

    it("returns the callback's value (sync and async)", async () => {
        const sync = Log.withRoot(factory, ["app"], () =>
            Log.withChild("a", () => 42),
        );
        expect(sync).toBe(42);

        const async = await Log.withRoot(factory, ["app"], async () =>
            Log.withChild("a", async () => 99),
        );
        expect(async).toBe(99);
    });

    it("propagates the child scope across awaits", async () => {
        await Log.withRoot(factory, ["app"], async () => {
            await Log.withChild("a", async () => {
                await new Promise((r) => setTimeout(r, 0));
                Log.info("after await");
            });
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "a"]);
    });

    it("removes its scope after a synchronous throw", () => {
        Log.withRoot(factory, ["app"], () => {
            const root = Log.instance();
            expect(() =>
                Log.withChild("auth", () => {
                    throw new Error("boom");
                }),
            ).toThrow("boom");
            // Back to the root after the failed child.
            expect(Log.instance()).toBe(root);
        });
    });

    it("reuses the cached child wrapper across repeated entries within the same root", () => {
        Log.withRoot(factory, ["app"], () => {
            let firstChild: Logger | undefined;
            let secondChild: Logger | undefined;

            Log.withChild("auth", () => {
                firstChild = Log.instance();
            });
            Log.withChild("auth", () => {
                secondChild = Log.instance();
            });

            // Two separate `withChild` entries produce the same underlying
            // wrapper, because LogTapeLogger.child() caches by subcategory.
            expect(firstChild).toBe(secondChild);
            // And it's the same wrapper as a direct call:
            expect(firstChild).toBe(Log.instance().child("auth"));
        });
    });

    it("the ambient logger inside withChild is identical to factory.get(root).child(sub)", () => {
        Log.withRoot(factory, ["app"], () => {
            Log.withChild("auth", () => {
                // The factory caches roots and LogTapeLogger caches
                // children, so the chain `factory.get(["app"]).child("auth")`
                // must be reference-equal to the ambient logger.
                expect(Log.instance()).toBe(factory.get(["app"]).child("auth"));
            });
        });
    });
});

// ---------------------------------------------------------------------------
// Log.get (LoggerFactory forwarding)
// ---------------------------------------------------------------------------

describe("Log.get (LoggerFactory forwarding)", () => {
    let cap: Capturer;

    beforeEach(async () => {
        cap = createCapturer();
        await setupCapture(cap);
    });

    it("forwards to the active factory's get(category) and returns identical wrappers (cache)", () => {
        Log.withRoot(factory, ["app"], () => {
            // The real LogTapeLoggerFactory caches wrappers per category, so
            // Log.get must round-trip to the same wrapper as a direct
            // factory.get call.
            const direct = factory.get(["app", "verbose"]);
            const viaLog = Log.get(["app", "verbose"]);
            expect(viaLog).toBe(direct);
        });
    });

    it("returns the same wrapper for repeated Log.get calls with the same category", () => {
        Log.withRoot(factory, ["app"], () => {
            const a = Log.get(["jobs"]);
            const b = Log.get(["jobs"]);
            expect(a).toBe(b);
        });
    });

    it("forwards to the active factory's get(category) (verified via spy)", () => {
        // A spying factory that delegates to the real one and records calls,
        // so we can verify that Log.get reaches the *factory* (not the
        // ambient logger's child(...)).
        const calls: unknown[] = [];
        const spied = {
            get(category: Parameters<typeof factory.get>[0]) {
                calls.push(category);
                return factory.get(category);
            },
        };

        Log.withRoot(spied, ["app"], () => {
            // First call inside withRoot's setup, then Log.get below.
            Log.get(["app", "verbose"]);
        });

        // Two calls expected: the implicit withRoot setup + the explicit get.
        expect(calls).toEqual([["app"], ["app", "verbose"]]);
    });

    it("returns a top-level logger for the requested category (not a child of the ambient logger)", () => {
        Log.withRoot(factory, ["app"], () => {
            // Even though the ambient logger is ['app'], asking for
            // ['app', 'verbose'] must yield the *factory's* top-level logger
            // for that category. Confirm both via reference identity (the
            // factory caches wrappers per category) and via the emitted
            // record's category.
            const viaLog = Log.get(["app", "verbose"]);
            expect(viaLog).toBe(factory.get(["app", "verbose"]));
            viaLog.info("hi");
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "verbose"]);
    });

    it("preserves the factory across withChild scopes", () => {
        const calls: unknown[] = [];
        const spied = {
            get(category: Parameters<typeof factory.get>[0]) {
                calls.push(category);
                return factory.get(category);
            },
        };

        Log.withRoot(spied, ["app"], () => {
            Log.withChild("inner", () => {
                // withChild does NOT call factory.get; it calls
                // logger.child(...). So the spy should not see another
                // entry until we explicitly call Log.get below.
                Log.get(["app"]);
            });
        });

        // Setup get(['app']) + the explicit Log.get(['app']) inside
        // withChild. `withChild` itself does not go through the factory.
        expect(calls).toEqual([["app"], ["app"]]);
    });

    it("Log can be passed wherever a LoggerFactory is expected", () => {
        Log.withRoot(factory, ["app"], () => {
            // Structural compatibility with LoggerFactory: `{ get(category) }`.
            const asFactory: { get(category: readonly [string]): unknown } =
                Log;
            const logger = asFactory.get(["app"]);
            // The factory's cache must make `Log.get` reference-stable,
            // so this is the very same wrapper as `factory.get(["app"])`.
            expect(logger).toBe(factory.get(["app"]));
        });
    });
});

// ---------------------------------------------------------------------------
// Log namespace (leveled API + general behaviour)
// ---------------------------------------------------------------------------

describe("Log namespace (ambient context, real end-to-end)", () => {
    let cap: Capturer;

    beforeEach(async () => {
        cap = createCapturer();
        await setupCapture(cap);
    });

    it("isInitialized is false outside withRoot", () => {
        expect(Log.isInitialized()).toBe(false);
    });

    it("isInitialized is true inside withRoot", () => {
        Log.withRoot(factory, ["app"], () => {
            expect(Log.isInitialized()).toBe(true);
        });
    });

    it("instance() returns the active logger inside withRoot", () => {
        // The factory caches wrappers per category, so `instance()` must
        // return the very same wrapper as `factory.get(["app"])`.
        Log.withRoot(factory, ["app"], () => {
            expect(Log.instance()).toBe(factory.get(["app"]));
        });
    });

    it("Log.info(message, properties) routes to the ambient logger's sink", () => {
        Log.withRoot(factory, ["app"], () => {
            Log.info("hello {name}", { name: "alice" });
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).level).toBe("info");
        expect(recordAt(cap).properties).toEqual({ name: "alice" });
    });

    it("Log.warn maps to logtape's 'warning' level", () => {
        Log.withRoot(factory, ["app"], () => {
            Log.warn("careful");
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).level).toBe("warning");
    });

    it("supports tagged templates inside withRoot", () => {
        Log.withRoot(factory, ["app"], () => {
            const v = "world";
            Log.info`hello ${v}`;
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).message).toEqual(["hello ", "world", ""]);
    });

    it("supports async compute inside an async withRoot", async () => {
        await Log.withRoot(factory, ["app"], async () => {
            const result = Log.info("ctx {user}", async () => ({
                user: "alice",
            }));
            expect(result).toBeInstanceOf(Promise);
            await result;
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).properties).toEqual({ user: "alice" });
    });

    it("ambient persists across awaits inside an async withRoot", async () => {
        await Log.withRoot(factory, ["app"], async () => {
            await new Promise((r) => setTimeout(r, 0));
            expect(Log.isInitialized()).toBe(true);
            Log.info("after await");
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).message).toEqual(["after await"]);
    });

    it("two concurrent withRoot calls do not bleed into each other", async () => {
        // Both Promise.all branches start outside any active withRoot, so
        // each can declare its own root independently.
        await Promise.all([
            Log.withRoot(factory, ["app"], async () => {
                await Log.withChild("a", async () => {
                    await new Promise((r) => setTimeout(r, 5));
                    Log.info("from a");
                });
            }),
            Log.withRoot(factory, ["app"], async () => {
                await Log.withChild("b", async () => {
                    await new Promise((r) => setTimeout(r, 1));
                    Log.info("from b");
                });
            }),
        ]);

        expect(cap.records).toHaveLength(2);
        const categories = cap.records.map((r) => r.category);
        expect(categories).toContainEqual(["app", "a"]);
        expect(categories).toContainEqual(["app", "b"]);
    });
});

// ---------------------------------------------------------------------------
// withLogTapeRootSync (sync helper from @/logtape)
// ---------------------------------------------------------------------------

describe("withLogTapeRootSync (sync helper from @/logtape)", () => {
    let cap: Capturer;

    beforeEach(() => {
        cap = createCapturer();
    });

    it("configures logtape, builds a factory, and scopes the root in one call", () => {
        const result = withLogTapeRootSync(["app"], captureConfig(cap), () => {
            Log.info("hello {name}", { name: "alice" });
            return 42;
        });

        expect(result).toBe(42);
        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).level).toBe("info");
        expect(recordAt(cap).category).toEqual(["app"]);
        expect(recordAt(cap).properties).toEqual({ name: "alice" });
    });

    it("supports a string root category", () => {
        withLogTapeRootSync("app", captureConfig(cap), () => {
            Log.info("hi");
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app"]);
    });

    it("Log.instance/withChild/get all work inside the helper", () => {
        withLogTapeRootSync(["app"], captureConfig(cap), () => {
            expect(Log.isInitialized()).toBe(true);

            // Forwards to the (newly constructed) factory — which is
            // category-cache-stable, so two get() calls return the same
            // wrapper.
            const a = Log.get(["app"]);
            const b = Log.get(["app"]);
            expect(a).toBe(b);

            Log.withChild("auth", () => {
                Log.info("from auth");
            });
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "auth"]);
    });

    it("the ambient is torn down after the callback returns", () => {
        withLogTapeRootSync(["app"], captureConfig(cap), () => {
            expect(Log.isInitialized()).toBe(true);
        });
        expect(Log.isInitialized()).toBe(false);
    });

    it("propagates synchronous throws and tears down the ambient", () => {
        expect(() =>
            withLogTapeRootSync(["app"], captureConfig(cap), () => {
                throw new Error("boom");
            }),
        ).toThrow("boom");
        expect(Log.isInitialized()).toBe(false);
    });

    it("refuses to nest inside an existing root (single-root invariant of Log.withRoot)", () => {
        withLogTapeRootSync(["app"], captureConfig(cap), () => {
            expect(() =>
                withLogTapeRootSync(["app"], captureConfig(cap), () => {
                    /* unreachable */
                }),
            ).toThrow(/single root/i);
        });
    });

    it("can be re-entered after the previous root completes", () => {
        withLogTapeRootSync(["app"], captureConfig(cap), () => {
            Log.info("first");
        });
        // Outside the previous root again — a fresh withLogTapeRootSync is
        // allowed and re-applies the config.
        withLogTapeRootSync(["app"], captureConfig(cap), () => {
            Log.info("second");
        });

        expect(cap.records).toHaveLength(2);
        expect(recordAt(cap).message).toEqual(["first"]);
        expect(recordAt(cap, 1).message).toEqual(["second"]);
    });

    it("the inner factory is reachable through Log.get and Log.instance", () => {
        withLogTapeRootSync(["app"], captureConfig(cap), () => {
            // The helper builds its own LogTapeLoggerFactory, so we can't
            // import the same instance — but Log.get reaches it, and the
            // family-wide cache means `Log.instance() === Log.get(["app"])`.
            expect(Log.instance()).toBe(Log.get(["app"]));
        });
    });
});

// ---------------------------------------------------------------------------
// withLogTapeRoot (async helper from @/logtape)
// ---------------------------------------------------------------------------

describe("withLogTapeRoot (async helper from @/logtape)", () => {
    let cap: Capturer;

    beforeEach(() => {
        cap = createCapturer();
    });

    it("returns a Promise that resolves to the callback's return value", async () => {
        const result = await withLogTapeRoot(
            ["app"],
            captureConfig(cap),
            async () => {
                Log.info("hello");
                return 99;
            },
        );

        expect(result).toBe(99);
        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).message).toEqual(["hello"]);
    });

    it("the ambient persists across awaits inside the async callback", async () => {
        await withLogTapeRoot(["app"], captureConfig(cap), async () => {
            await new Promise((r) => setTimeout(r, 0));
            expect(Log.isInitialized()).toBe(true);
            Log.info("after await");
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).message).toEqual(["after await"]);
    });

    it("supports Log.withChild + async compute inside the async callback", async () => {
        await withLogTapeRoot(["app"], captureConfig(cap), async () => {
            await Log.withChild("auth", async () => {
                const result = Log.info("ctx {user}", async () => ({
                    user: "alice",
                }));
                expect(result).toBeInstanceOf(Promise);
                await result;
            });
        });

        expect(cap.records).toHaveLength(1);
        expect(recordAt(cap).category).toEqual(["app", "auth"]);
        expect(recordAt(cap).properties).toEqual({ user: "alice" });
    });

    it("rejects when the async callback rejects, and tears down the ambient", async () => {
        await expect(
            withLogTapeRoot(["app"], captureConfig(cap), async () => {
                throw new Error("boom");
            }),
        ).rejects.toThrow("boom");
        expect(Log.isInitialized()).toBe(false);
    });

    it("refuses to nest inside an existing root", async () => {
        await withLogTapeRoot(["app"], captureConfig(cap), async () => {
            await expect(
                withLogTapeRoot(["app"], captureConfig(cap), async () => {
                    /* unreachable */
                }),
            ).rejects.toThrow(/single root/i);
        });
    });
});
