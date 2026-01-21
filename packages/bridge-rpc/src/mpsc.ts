import { Mutex } from "async-mutex";

export class Mpsc<T> {
    public readonly sender: MpscSender<T>;
    public readonly receiver: MpscReceiver<T>;

    constructor() {
        const state: MpscState<T> = {
            queue: [],
            waiters: [],
            closed: false,
        };

        this.sender = new MpscSender(state);
        this.receiver = new MpscReceiver(state);
    }
}

type Waiter<T> = (value: IteratorResult<T>) => void;

type MpscState<T> = {
    queue: T[];
    waiters: Waiter<T>[];
    closed: boolean;
};

export class MpscReceiver<T> implements AsyncIterable<T> {
    private mutex = new Mutex();

    constructor(private readonly state: MpscState<T>) {}

    /**
     * Receives the next value. If the queue is empty,
     * it returns a promise that resolves when a value is sent.
     */
    public next(): Promise<IteratorResult<T>> {
        return this.mutex.runExclusive(async () => {
            if (this.state.queue.length > 0) {
                // biome-ignore lint/style/noNonNullAssertion: allow
                return { done: false, value: this.state.queue.shift()! };
            }

            if (this.state.closed) {
                return { done: true, value: undefined };
            }

            return new Promise((resolve) => {
                this.state.waiters.push(resolve);
            });
        });
    }

    public async receive(): Promise<T | undefined> {
        const result = await this.next();

        return result.done ? undefined : result.value;
    }

    /**
     * Enables usage in `for await (const val of rx)`
     */
    async *[Symbol.asyncIterator]() {
        while (true) {
            const result = await this.next();
            if (result.done) break;
            yield result.value;
        }
    }

    public close() {
        this.state.closed = true;
        while (this.state.waiters.length > 0) {
            // biome-ignore lint/style/noNonNullAssertion: allow
            const resolve = this.state.waiters.shift()!;
            resolve({ done: true, value: undefined });
        }
    }
}

export class MpscSender<T> {
    constructor(private readonly state: MpscState<T>) {}

    public send(value: T) {
        if (this.state.closed) {
            throw new MpscClosedError();
        }

        if (this.state.waiters.length > 0) {
            // biome-ignore lint/style/noNonNullAssertion: allow
            const resolve = this.state.waiters.shift()!;
            resolve({ done: false, value });
        } else {
            this.state.queue.push(value);
        }
    }

    public close() {
        this.state.closed = true;
        // Wake up all pending receivers to tell them it's over
        while (this.state.waiters.length > 0) {
            // biome-ignore lint/style/noNonNullAssertion: allow
            const resolve = this.state.waiters.shift()!;
            resolve({ done: true, value: undefined });
        }
    }

    /**
     * Allows creating multiple producers from one channel
     */
    public clone(): MpscSender<T> {
        return new MpscSender(this.state);
    }
}

export class MpscClosedError extends Error {
    constructor() {
        super("Mpsc is closed");
        super.name = "MpscClosedError";
    }
}
