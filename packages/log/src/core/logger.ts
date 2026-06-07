import type { LogLevel } from "./level";

// Cached reference to the AsyncFunction constructor so we can identify
// `async function`/`async () => {}` literals at runtime without invoking them.
//
// NOTE: `Function.prototype.bind` produces a plain function, so a bound async
// function will *not* match this check and will be treated as a synchronous
// compute. Callers that wrap async functions should pass them directly.
const AsyncFunction = (async () => {
    /* noop */
}).constructor as new (
    ...args: unknown[]
) => unknown;

function isAsyncFunction(
    value: unknown,
): value is () => Promise<Record<string, unknown>> {
    return typeof value === "function" && value instanceof AsyncFunction;
}

export interface LeveledLogFunction {
    (message: string, properties?: Record<string, unknown>): void;
    (message: string, computeProperties?: () => Record<string, unknown>): void;
    (
        message: string,
        computeProperties?: () => Promise<Record<string, unknown>>,
    ): Promise<void>;
    (template: TemplateStringsArray, ...args: unknown[]): void;
}

export interface LogFunction {
    (
        level: LogLevel,
        message: string,
        properties?: Record<string, unknown>,
    ): void;
    (
        level: LogLevel,
        message: string,
        computeProperties?: () => Record<string, unknown>,
    ): void;
    (
        level: LogLevel,
        message: string,
        computeProperties?: () => Promise<Record<string, unknown>>,
    ): Promise<void>;
    (level: LogLevel): LeveledLogFunction;
}

export type ChildFunction = (subcategory: CategoryParam) => Logger;
export type WithFunction = (properties: Record<string, unknown>) => Logger;
export type EnabledFunction = (level: LogLevel) => boolean;

export type CategoryParam =
    | string
    | readonly [string]
    | readonly [string, ...string[]];

export interface Logger {
    readonly enabled: EnabledFunction;

    readonly child: ChildFunction;
    readonly parent: Logger | null;

    readonly with: WithFunction;

    readonly log: LogFunction;
    readonly error: LeveledLogFunction;
    readonly warn: LeveledLogFunction;
    readonly info: LeveledLogFunction;
    readonly debug: LeveledLogFunction;
    readonly trace: LeveledLogFunction;
}

export abstract class AbstractLogger implements Logger {
    protected abstract logEager: LogEager;

    protected abstract logLazy: LogLazy;

    protected abstract logLazyAsync: LogLazyAsync;

    protected abstract logTemplate: LogTemplate;

    abstract enabled(level: LogLevel): boolean;

    abstract child(subcategory: CategoryParam): Logger;

    abstract parent: Logger | null;

    abstract with(properties: Record<string, unknown>): Logger;

    protected dispatch(
        level: LogLevel,
        messageOrTemplate: string | TemplateStringsArray,
        rest: unknown[],
    ): void | Promise<void> {
        if (typeof messageOrTemplate === "string") {
            const propertiesOrCompute = rest[0];
            if (typeof propertiesOrCompute === "function") {
                if (isAsyncFunction(propertiesOrCompute)) {
                    return this.logLazyAsync(
                        level,
                        messageOrTemplate,
                        propertiesOrCompute,
                    );
                }
                this.logLazy(
                    level,
                    messageOrTemplate,
                    propertiesOrCompute as () => Record<string, unknown>,
                );
                return;
            }
            this.logEager(
                level,
                messageOrTemplate,
                propertiesOrCompute as Record<string, unknown> | undefined,
            );
            return;
        }
        this.logTemplate(level, messageOrTemplate, rest);
    }

    // Arrow-bound so `const { log } = logger;` works.
    //
    // When called with only a level, returns the matching leveled log
    // function (`this.error`, `this.warn`, ...). Those fields are arrow-bound
    // and referentially stable per instance, so `log(level)` is
    // allocation-free on hot paths and `log(level) === log(level)`.
    //
    // When called with `(level, message, asyncCompute)`, the result of
    // `dispatch` (a `Promise<void>`) is returned to the caller so it can be
    // awaited.
    log: LogFunction = ((level: LogLevel, ...rest: unknown[]) => {
        if (rest.length === 0) {
            return this[level];
        }

        const [message, propertiesOrCompute] = rest;
        if (typeof message !== "string") {
            throw new TypeError(
                `AbstractLogger.log: expected a string message, received ${
                    message === null ? "null" : typeof message
                }`,
            );
        }

        return this.dispatch(level, message, [propertiesOrCompute]);
    }) as LogFunction;

    error: LeveledLogFunction = ((
        messageOrTemplate: string | TemplateStringsArray,
        ...rest: unknown[]
    ) => this.dispatch("error", messageOrTemplate, rest)) as LeveledLogFunction;

    warn: LeveledLogFunction = ((
        messageOrTemplate: string | TemplateStringsArray,
        ...rest: unknown[]
    ) => this.dispatch("warn", messageOrTemplate, rest)) as LeveledLogFunction;

    info: LeveledLogFunction = ((
        messageOrTemplate: string | TemplateStringsArray,
        ...rest: unknown[]
    ) => this.dispatch("info", messageOrTemplate, rest)) as LeveledLogFunction;

    debug: LeveledLogFunction = ((
        messageOrTemplate: string | TemplateStringsArray,
        ...rest: unknown[]
    ) => this.dispatch("debug", messageOrTemplate, rest)) as LeveledLogFunction;

    trace: LeveledLogFunction = ((
        messageOrTemplate: string | TemplateStringsArray,
        ...rest: unknown[]
    ) => this.dispatch("trace", messageOrTemplate, rest)) as LeveledLogFunction;
}

export type LogEager = (
    level: LogLevel,
    message: string,
    properties?: Record<string, unknown>,
) => void;

export type LogLazy = (
    level: LogLevel,
    message: string,
    computeProperties?: () => Record<string, unknown>,
) => void;

export type LogLazyAsync = (
    level: LogLevel,
    message: string,
    computeProperties: () => Promise<Record<string, unknown>>,
) => Promise<void>;

export type LogTemplate = (
    level: LogLevel,
    template: TemplateStringsArray,
    args: unknown[],
) => void;

export interface LoggerFactory {
    get(category: CategoryParam): Logger;
}
