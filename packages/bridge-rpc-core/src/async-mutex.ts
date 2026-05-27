import { Mutex } from "async-mutex";

export class AsyncMutex<T> {
    private readonly mutex = new Mutex();

    constructor(private readonly value: T) {}

    public runExclusive<R>(
        fn: (value: T) => R | Promise<R>,
        priority?: number,
    ): Promise<R> {
        return this.mutex.runExclusive(() => fn(this.value), priority);
    }
}
