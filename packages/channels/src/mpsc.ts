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
    // a queue of functions registered to receive values, in the order they were awaited (FIFO)
    waiters: Waiter<T>[];
    closed: boolean;
};

export class MpscReceiver<T> implements AsyncIterable<T> {
    constructor(private readonly state: MpscState<T>) {}

    /**
     * Receives the next value. If the queue is empty,
     * it returns a promise that resolves when a value is sent.
     */
    public next(): Promise<IteratorResult<T>> {
        // if there are values in the queue, return the first one
        if (this.state.queue.length > 0) {
            return Promise.resolve({
                done: false,
                // biome-ignore lint/style/noNonNullAssertion: allow
                value: this.state.queue.shift()!,
            });
        }

        // if the channel is closed, return a promise that resolves with done: true
        if (this.state.closed) {
            return Promise.resolve({ done: true, value: undefined });
        }

        // if there is no value in the queue and the channel is not closed, return a promise that resolves when a value is sent
        return new Promise((resolve) => {
            this.state.waiters.push(resolve);
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

        // let all pending receivers know that the channel is closed
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

export function mpsc<T>(): Mpsc<T> {
    return new Mpsc();
}
