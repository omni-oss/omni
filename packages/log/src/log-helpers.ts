import type { LeveledLogFunction, Logger, LogLevel } from "./core";

/**
 * The five leveled log methods every facade exposes. Mirrors the leveled
 * surface of {@link Logger} so a facade can be assembled from
 * {@link createLeveledForwarders} alone.
 */
export interface LeveledForwarders {
    error: LeveledLogFunction;
    warn: LeveledLogFunction;
    info: LeveledLogFunction;
    debug: LeveledLogFunction;
    trace: LeveledLogFunction;
}

/**
 * Build a complete set of leveled forwarders that delegate to the logger
 * returned by `getLogger()` *at call time* (so the underlying logger can
 * change between calls — e.g. an ambient-scoped facade).
 *
 * Each forwarder preserves all `LeveledLogFunction` overloads: string
 * messages, sync/async compute callbacks, and tagged templates. Async
 * callbacks' `Promise<void>` returns flow through to the caller so they
 * can be awaited.
 */
export function createLeveledForwarders(
    getLogger: () => Logger,
): LeveledForwarders {
    function build(level: LogLevel): LeveledLogFunction {
        const fn = (
            messageOrTemplate: string | TemplateStringsArray,
            ...rest: unknown[]
        ): unknown => {
            const target = getLogger()[level] as (
                messageOrTemplate: string | TemplateStringsArray,
                ...rest: unknown[]
            ) => unknown;
            return target(messageOrTemplate, ...rest);
        };
        // The cast through `unknown` is necessary because TypeScript can't
        // express "preserve overloaded signatures when forwarding via a
        // rest parameter".
        return fn as unknown as LeveledLogFunction;
    }

    return {
        error: build("error"),
        warn: build("warn"),
        info: build("info"),
        debug: build("debug"),
        trace: build("trace"),
    };
}
