/**
 * Cross-runtime "ambient context" abstraction.
 *
 * Models the same shape as Node's {@link import("node:async_hooks").AsyncLocalStorage}
 * (a {@link run} method that scopes a value for the duration of a callback,
 * plus {@link getStore} that retrieves the active value), but selects the
 * underlying implementation based on the detected runtime:
 *
 * 1. **Node-like runtimes** (Node, Bun, Deno, edge runtimes that expose Node's
 *    `node:async_hooks`) get the real {@link AsyncLocalStorage}. This is the
 *    only implementation that survives every `await`/microtask boundary.
 * 2. **Edge runtimes that expose `globalThis.AsyncLocalStorage`** (Cloudflare
 *    Workers etc.) use that constructor directly.
 * 3. **Pure browser** (no `AsyncLocalStorage` available) falls back to a
 *    synchronous stack-based implementation that *does* work for synchronous
 *    code and naively-nested async code, but cannot guarantee correct
 *    propagation across interleaved promise chains. A one-time warning is
 *    emitted to make this caveat explicit.
 */

// Use a structural type for the ALS constructor so the runtime cast does not
// pull in `@types/node` when the consumer is purely a browser build.
type AsyncLocalStorageLike<T> = {
    run<R>(value: T, fn: () => R): R;
    getStore(): T | undefined;
};
type AsyncLocalStorageCtor = new <T>() => AsyncLocalStorageLike<T>;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface AmbientContext<T> {
    /**
     * Run `fn` with `value` as the active ambient store. The store is
     * scoped to the callback (and all calls reachable from it) and is
     * removed once the callback completes.
     *
     * If `fn` returns a Promise, the resolved value is forwarded; the
     * fallback implementation also waits for that Promise to settle before
     * popping the value from its stack.
     */
    run<R>(value: T, fn: () => R): R;

    /**
     * The current ambient value, or `undefined` if there is no active
     * `run` on the call stack.
     */
    getStore(): T | undefined;
}

/**
 * Discriminator for the chosen platform implementation. Useful for tests
 * and for diagnostic logging.
 */
export type AmbientContextKind = "async-local-storage" | "stack-fallback";

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

let resolvedKind: AmbientContextKind = "stack-fallback";
let alsCtor: AsyncLocalStorageCtor | null = null;

// 1) Globally-exposed AsyncLocalStorage (Cloudflare Workers, Bun's web mode,
//    some edge runtimes).
{
    const candidate = (globalThis as Record<string, unknown>).AsyncLocalStorage;
    if (typeof candidate === "function") {
        alsCtor = candidate as unknown as AsyncLocalStorageCtor;
        resolvedKind = "async-local-storage";
    }
}

// 2) Node-like runtime: import `node:async_hooks` lazily. Top-level `await`
//    keeps detection synchronous from the perspective of any consumer that
//    imports this module via `await import(...)` or static ESM (vite/esbuild
//    wrap CJS output appropriately for `target: ESNext`).
if (alsCtor === null) {
    const isNodeLike =
        typeof (globalThis as { process?: { versions?: { node?: unknown } } })
            .process?.versions?.node === "string";
    if (isNodeLike) {
        try {
            // Dynamic import keeps browser bundles from statically pulling in
            // a Node-only built-in.
            const mod = (await import("node:async_hooks")) as {
                AsyncLocalStorage: AsyncLocalStorageCtor;
            };
            alsCtor = mod.AsyncLocalStorage;
            resolvedKind = "async-local-storage";
        } catch {
            // Defensive: if a Node-shaped global lacks async_hooks (extremely
            // unusual), fall through to the stack-based fallback below.
        }
    }
}

let warnedAboutFallback = false;
function warnFallbackOnce(): void {
    if (warnedAboutFallback) return;
    warnedAboutFallback = true;
    // eslint-disable-next-line no-console
    console.warn(
        "[@omni-oss/log] AsyncLocalStorage is not available in this runtime; " +
            "AmbientContext is using a stack-based fallback that does not " +
            "correctly propagate across interleaved async boundaries. " +
            "Use this only in environments without true async-context support.",
    );
}

// ---------------------------------------------------------------------------
// Implementations
// ---------------------------------------------------------------------------

function createAlsBacked<T>(Ctor: AsyncLocalStorageCtor): AmbientContext<T> {
    const als = new Ctor<T>();
    return {
        run<R>(value: T, fn: () => R): R {
            return als.run(value, fn);
        },
        getStore(): T | undefined {
            return als.getStore();
        },
    };
}

/**
 * Synchronous stack-based fallback. Sufficient for purely-synchronous code
 * and for strictly-nested `await` flows in single-task environments. Breaks
 * down once independent async tasks interleave.
 */
function createStackFallback<T>(): AmbientContext<T> {
    const stack: T[] = [];
    return {
        run<R>(value: T, fn: () => R): R {
            warnFallbackOnce();
            stack.push(value);
            let result: R;
            try {
                result = fn();
            } catch (err) {
                stack.pop();
                throw err;
            }
            if (
                result !== null &&
                typeof result === "object" &&
                typeof (result as { then?: unknown }).then === "function"
            ) {
                // Best-effort: keep the value on the stack until the promise
                // settles. Concurrent runs on the same fallback may observe
                // each other's values; that's the documented limitation.
                return (result as unknown as Promise<unknown>).finally(() => {
                    stack.pop();
                }) as unknown as R;
            }
            stack.pop();
            return result;
        },
        getStore(): T | undefined {
            return stack[stack.length - 1];
        },
    };
}

// ---------------------------------------------------------------------------
// Public factory
// ---------------------------------------------------------------------------

/**
 * Build a fresh {@link AmbientContext}. Each call returns an independent
 * instance (independent stores), matching the semantics of constructing a
 * new `AsyncLocalStorage`.
 */
export function createAmbientContext<T>(): AmbientContext<T> {
    if (alsCtor !== null) {
        return createAlsBacked<T>(alsCtor);
    }
    return createStackFallback<T>();
}

/**
 * Returns which implementation was selected at module load. Mainly intended
 * for diagnostic / test usage.
 */
export function ambientContextKind(): AmbientContextKind {
    return resolvedKind;
}
