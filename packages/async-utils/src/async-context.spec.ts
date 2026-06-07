import { AsyncLocalStorage } from "node:async_hooks";
import { describe, expect, it } from "vitest";
import {
    asyncContextSnapshotSupported,
    bindAsyncContext,
    captureAsyncContext,
} from "./async-context";

describe("async-context", () => {
    it("reports snapshot support under Node", () => {
        // Vitest runs on Node, where `AsyncLocalStorage.snapshot()` has
        // been available since v18.16 / v19.8.
        expect(asyncContextSnapshotSupported()).toBe(true);
    });

    describe("captureAsyncContext", () => {
        it("re-enters the captured store inside a deferred runner", async () => {
            const als = new AsyncLocalStorage<string>();
            let runner!: <R>(fn: () => R) => R;

            await als.run("alpha", async () => {
                runner = captureAsyncContext();
            });

            // The runner is invoked *outside* the original `als.run`,
            // and there is no active `als.run` on the stack here.
            expect(als.getStore()).toBeUndefined();
            expect(runner(() => als.getStore())).toBe("alpha");
            // The outer scope is left untouched.
            expect(als.getStore()).toBeUndefined();
        });

        it("propagates across multiple AsyncLocalStorage instances", async () => {
            const a = new AsyncLocalStorage<number>();
            const b = new AsyncLocalStorage<string>();
            let runner!: <R>(fn: () => R) => R;

            await a.run(7, async () => {
                await b.run("seven", async () => {
                    runner = captureAsyncContext();
                });
            });

            const seen = runner(() => ({
                a: a.getStore(),
                b: b.getStore(),
            }));
            expect(seen).toEqual({ a: 7, b: "seven" });
        });

        it("forwards the runner callback's return value", () => {
            const runner = captureAsyncContext();
            expect(runner(() => 42)).toBe(42);
        });

        it("captures fresh state on each call", async () => {
            const als = new AsyncLocalStorage<string>();
            let r1!: <R>(fn: () => R) => R;
            let r2!: <R>(fn: () => R) => R;

            await als.run("first", async () => {
                r1 = captureAsyncContext();
            });
            await als.run("second", async () => {
                r2 = captureAsyncContext();
            });

            expect(r1(() => als.getStore())).toBe("first");
            expect(r2(() => als.getStore())).toBe("second");
        });
    });

    describe("bindAsyncContext", () => {
        it("invokes the wrapped function with the captured store", async () => {
            const als = new AsyncLocalStorage<string>();
            let bound!: () => string | undefined;

            await als.run("bravo", async () => {
                bound = bindAsyncContext(() => als.getStore());
            });

            expect(als.getStore()).toBeUndefined();
            expect(bound()).toBe("bravo");
        });

        it("forwards arguments and the return value", async () => {
            const als = new AsyncLocalStorage<string>();
            let bound!: (a: number, b: number) => string;

            await als.run("scope", async () => {
                bound = bindAsyncContext(
                    (a: number, b: number) => `${als.getStore()}:${a + b}`,
                );
            });

            expect(bound(2, 3)).toBe("scope:5");
        });

        it("propagates the store across awaits inside the wrapped function", async () => {
            const als = new AsyncLocalStorage<string>();
            let bound!: () => Promise<string | undefined>;

            await als.run("async-scope", async () => {
                bound = bindAsyncContext(async () => {
                    await new Promise((r) => setTimeout(r, 0));
                    return als.getStore();
                });
            });

            await expect(bound()).resolves.toBe("async-scope");
        });

        it("repeated invocations all observe the same captured store", async () => {
            const als = new AsyncLocalStorage<number>();
            let bound!: () => number | undefined;

            await als.run(99, async () => {
                bound = bindAsyncContext(() => als.getStore());
            });

            expect(bound()).toBe(99);
            expect(bound()).toBe(99);
            expect(bound()).toBe(99);
        });
    });
});
