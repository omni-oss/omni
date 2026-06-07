/**
 * Cross-runtime helpers for capturing and re-entering the current
 * `AsyncLocalStorage` context when a callback's invocation site is
 * decoupled from its registration site.
 *
 * # Why this exists
 *
 * Node's {@link import("node:async_hooks").AsyncLocalStorage} threads a
 * store through the natural async-hook chain: any `await`/microtask
 * reachable from inside `als.run(value, fn)` observes the store. That
 * works great for code that *runs* inside `run()`, but it breaks down for
 * the common pattern where:
 *
 *   1. Some long-lived object (a stream, an event emitter, a worker) is
 *      constructed *outside* any `als.run(...)` scope, capturing the
 *      empty async context at construction time.
 *   2. A consumer registers a callback on that object from *inside*
 *      `als.run(...)` (e.g. `transport.onReceive(handler)` while a
 *      logger root is active).
 *   3. The object later invokes the callback from its original (empty)
 *      async context — typically via an internal stream pump or queued
 *      microtask. The callback runs without the store, even though the
 *      registration was clearly intentional and made inside `run()`.
 *
 * The remedy is to *snapshot* the async context at registration time and
 * re-enter it on every invocation. Node exposes
 * {@link AsyncLocalStorage.snapshot} for exactly this purpose: it
 * captures every currently-active ALS — across all instances at once —
 * and returns a runner that restores them. This module wraps that
 * primitive in a small cross-runtime API, with a no-op identity fallback
 * for runtimes that lack it.
 */

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/**
 * A function that runs `fn` with a previously-captured async context
 * restored. Returns whatever `fn` returns.
 *
 * Created by {@link captureAsyncContext}.
 */
export type AsyncContextRunner = <R>(fn: () => R) => R;

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

type AsyncLocalStorageStatic = {
    snapshot?: () => <R>(fn: () => R, ...args: unknown[]) => R;
};

/**
 * Resolved at module load. `null` if `AsyncLocalStorage.snapshot` is
 * unavailable in this runtime, in which case {@link captureAsyncContext}
 * falls back to an identity runner.
 */
let snapshotFactory: (() => AsyncContextRunner) | null = null;

// 1) Globally-exposed AsyncLocalStorage (Cloudflare Workers, Bun's web
//    mode, some edge runtimes).
{
    const globalAls = (
        globalThis as { AsyncLocalStorage?: AsyncLocalStorageStatic }
    ).AsyncLocalStorage;
    if (globalAls && typeof globalAls.snapshot === "function") {
        const snap = globalAls.snapshot.bind(globalAls);
        snapshotFactory = () => snap() as AsyncContextRunner;
    }
}

// 2) Node-like runtime: import `node:async_hooks` lazily. Top-level
//    `await` keeps detection synchronous from the perspective of any
//    consumer that imports this module via static ESM (vite/esbuild
//    handle this fine for `target: ESNext`).
if (snapshotFactory === null) {
    const isNodeLike =
        typeof (globalThis as { process?: { versions?: { node?: unknown } } })
            .process?.versions?.node === "string";
    if (isNodeLike) {
        try {
            const mod = (await import("node:async_hooks")) as {
                AsyncLocalStorage: AsyncLocalStorageStatic;
            };
            const cls = mod.AsyncLocalStorage;
            if (typeof cls.snapshot === "function") {
                const snap = cls.snapshot.bind(cls);
                snapshotFactory = () => snap() as AsyncContextRunner;
            }
        } catch {
            // Defensive: fall through to the identity runner below.
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Whether this runtime supports `AsyncLocalStorage.snapshot()`. When
 * `false`, {@link captureAsyncContext} returns an identity runner and
 * {@link bindAsyncContext} is effectively a passthrough.
 *
 * Mainly intended for diagnostic / test usage.
 */
export function asyncContextSnapshotSupported(): boolean {
    return snapshotFactory !== null;
}

/**
 * Capture the currently-active async context (i.e. every active
 * `AsyncLocalStorage` store) and return a {@link AsyncContextRunner}
 * that re-enters it.
 *
 * Each call returns an independent snapshot. The snapshot is taken at
 * the moment of the call, *not* at the moment the runner is invoked, so
 * call this *inside* the `als.run(...)` scope you want to preserve.
 *
 * If the runtime does not support snapshots, returns an identity runner
 * (`fn => fn()`). Code that relies on this helper should treat the
 * fallback as best-effort.
 */
export function captureAsyncContext(): AsyncContextRunner {
    if (snapshotFactory !== null) {
        return snapshotFactory();
    }
    return identityRunner;
}

/**
 * Wrap `fn` so that every invocation runs with the async context that
 * was active at the time `bindAsyncContext` was called.
 *
 * The returned function preserves `fn`'s parameter and return types and
 * forwards arguments verbatim. `this` is *not* preserved — pre-bind it
 * (e.g. `bindAsyncContext(obj.method.bind(obj))`) if you need a
 * particular receiver.
 *
 * Equivalent to `const runner = captureAsyncContext(); (...args) =>
 * runner(() => fn(...args))`, but slightly cheaper (skips closure
 * allocation when the runtime has no snapshot support).
 */
export function bindAsyncContext<TArgs extends unknown[], TReturn>(
    fn: (...args: TArgs) => TReturn,
): (...args: TArgs) => TReturn {
    if (snapshotFactory === null) {
        return fn;
    }
    const runner = snapshotFactory();
    return (...args: TArgs) => runner(() => fn(...args));
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

const identityRunner: AsyncContextRunner = (fn) => fn();
