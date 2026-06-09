import {
    AbstractLogger,
    type CategoryParam,
    type LogEager,
    type Logger,
    type LoggerFactory,
    type LogLazy,
    type LogLazyAsync,
    type LogLevel,
    type LogTemplate,
} from "..";

export class NoopLogger extends AbstractLogger {
    constructor(
        private readonly category: readonly string[],
        public readonly parent: Logger | null = null,
    ) {
        super();
    }

    protected override logEager: LogEager = () => {};
    protected override logLazy: LogLazy = () => {};
    protected override logLazyAsync: LogLazyAsync = () => Promise.resolve();
    protected override logTemplate: LogTemplate = () => {};
    override child(subcategory: CategoryParam): Logger {
        const newCategory = [...this.category, ...toCategory(subcategory)];
        return new NoopLogger(newCategory, this);
    }
    override enabled(_level: LogLevel): boolean {
        return false;
    }
    override with(_properties: Record<string, unknown>): Logger {
        return this;
    }
}

export class NoopLoggerFactory implements LoggerFactory {
    get(category: CategoryParam): Logger {
        return new NoopLogger(toCategory(category));
    }
}

function toCategory(category: CategoryParam): readonly string[] {
    return Array.isArray(category)
        ? category
        : typeof category === "string"
          ? [category]
          : category;
}
