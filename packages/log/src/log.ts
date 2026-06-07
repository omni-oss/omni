import { createAmbientContext } from "./ambient-context";
import type {
    CategoryParam,
    LeveledLogFunction,
    Logger,
    LoggerFactory,
} from "./core";
import { createLeveledForwarders } from "./log-helpers";

/**
 * What we keep in the ambient store. Both the originating
 * {@link LoggerFactory} and the currently-scoped {@link Logger} are needed:
 *
 * - The factory is what {@link Log.get} forwards to (it must be the
 *   *factory's* `get`, so a category lookup yields a top-level logger of
 *   that category — not a child of whatever logger happens to be on top of
 *   the call stack).
 * - The logger is the resolution target for `Log.instance` and for the
 *   leveled helpers.
 */
interface AmbientLogState {
    readonly factory: LoggerFactory;
    readonly logger: Logger;
}

const ambient = createAmbientContext<AmbientLogState>();

function requireState(operation: string): AmbientLogState {
    const state = ambient.getStore();
    if (state === undefined) {
        throw new Error(
            `Logger is not initialized. Wrap your code with Log.withRoot(factory, category, ...) before calling ${operation}.`,
        );
    }
    return state;
}

/**
 * Facade for the ambient logger.
 *
 * The {@link Log} namespace exposes the same leveled API (`Log.error`,
 * `Log.warn`, …) as the underlying {@link Logger}, but resolves the actual
 * target dynamically via {@link AmbientContext}. To make a logger active
 * for a region of code, wrap that code in {@link Log.withRoot}; nested
 * scopes can re-categorise via {@link Log.withChild}.
 *
 * `Log` itself implements the {@link LoggerFactory} contract via
 * {@link Log.get}, forwarding to the factory passed into the active
 * `withRoot` call.
 *
 * @example
 * ```ts
 * import { Log, LogTapeLoggerFactory } from "@omni-oss/log";
 *
 * const factory = new LogTapeLoggerFactory();
 *
 * Log.withRoot(factory, ["app"], () => {
 *     Log.info("hello {name}", { name: "alice" });
 *
 *     Log.withChild("auth", () => {
 *         // category here is ["app", "auth"]
 *         Log.warn("session expiring");
 *     });
 *
 *     // `Log.get` forwards to the factory, *not* the ambient logger,
 *     // so this returns a top-level logger for ["jobs", "cleanup"]:
 *     const jobs = Log.get(["jobs", "cleanup"]);
 *     jobs.info("kicked off");
 * });
 *
 * // Async callbacks are supported; the returned promise mirrors the
 * // callback's return type.
 * await Log.withRoot(factory, ["app"], async () => {
 *     await something();
 *     Log.info("done");
 * });
 * ```
 */
export namespace Log {
    /**
     * Whether a root logger is currently in scope (i.e. we are inside some
     * `Log.withRoot(...)` call).
     */
    export function isInitialized(): boolean {
        return ambient.getStore() !== undefined;
    }

    /**
     * Returns the logger that is currently in scope. Throws if called
     * outside a `Log.withRoot(...)` block.
     */
    export function instance(): Logger {
        return requireState("Log.instance()").logger;
    }

    /**
     * Forwards to the active {@link LoggerFactory}'s `get(category)`. Throws
     * if called outside a `Log.withRoot(...)` block.
     *
     * Mirrors the {@link LoggerFactory} contract so `Log` itself can be
     * passed wherever a `LoggerFactory` is expected.
     */
    export function get(category: CategoryParam): Logger {
        return requireState("Log.get(...)").factory.get(category);
    }

    /**
     * Establish the root logger for the current execution context.
     *
     * Builds the root via `factory.get(category)`, scopes it for the
     * duration of `fn`, then restores the previous (empty) ambient state.
     * On Node-like runtimes the scoping flows through every `await`/
     * microtask inside `fn`.
     *
     * **An application must declare exactly one root.** Calling `withRoot`
     * while another `withRoot` is already active throws. Use
     * {@link Log.withChild} to refine the category for a nested scope.
     *
     * The return value of `fn` is forwarded as-is. If `fn` is `async`,
     * `withRoot` returns the same `Promise`.
     */
    export function withRoot<R>(
        factory: LoggerFactory,
        category: CategoryParam,
        fn: () => R,
    ): R {
        if (ambient.getStore() !== undefined) {
            throw new Error(
                "A root logger is already active in this execution context. An application must declare a single root via Log.withRoot(...); use Log.withChild(category, ...) to derive nested scopes.",
            );
        }
        const logger = factory.get(category);
        return ambient.run({ factory, logger }, fn);
    }

    /**
     * Run `fn` with a child of the ambient logger as the new ambient
     * logger. The {@link LoggerFactory} carried in the ambient state is
     * preserved so {@link Log.get} continues to work.
     *
     * Throws if there is no active {@link Log.withRoot} on the call stack.
     *
     * Like `withRoot`, the return value of `fn` is forwarded as-is and
     * async callbacks scope correctly across `await`s.
     */
    export function withChild<R>(category: CategoryParam, fn: () => R): R {
        const state = requireState("Log.withChild(...)");
        const child = state.logger.child(category);
        return ambient.run({ factory: state.factory, logger: child }, fn);
    }

    const forwarders = createLeveledForwarders(instance);

    export const error: LeveledLogFunction = forwarders.error;
    export const warn: LeveledLogFunction = forwarders.warn;
    export const info: LeveledLogFunction = forwarders.info;
    export const debug: LeveledLogFunction = forwarders.debug;
    export const trace: LeveledLogFunction = forwarders.trace;
}
