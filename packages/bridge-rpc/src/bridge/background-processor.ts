export class BackgroundProcessor {
    private _tasks: Set<Promise<unknown>> = new Set();
    private _errors: Record<string, Error> = {};

    public queue<R>(task: Promise<R>): string {
        const id = crypto.randomUUID();
        this._tasks.add(
            task
                .catch((e) => {
                    if (e instanceof Error) {
                        this._errors[id] = e;
                    } else {
                        this._errors[id] = new Error(e);
                    }
                })
                .finally(() => {
                    this._tasks.delete(task);
                }),
        );

        return id;
    }

    public async awaitAll() {
        await Promise.all(this._tasks);

        const errors = Object.values(this._errors);
        if (errors.length > 0) {
            throw new BackgroundProcessorCompoundError(errors);
        }
    }

    public getError(id: string) {
        return this._errors[id];
    }

    public hasError(id: string) {
        return this._errors[id] !== undefined;
    }

    public clearErrors() {
        this._errors = {};
    }
}

export class BackgroundProcessorCompoundError extends Error {
    constructor(public readonly errors: Error[]) {
        super("there were errors in the background processor");
        this.name = "BackgroundProcessorCompoundError";
    }
}
