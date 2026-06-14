/**
 * The result of {@link MpscSender.send}.
 *
 * For an **unbounded** channel (`C` is `undefined`) `send` is synchronous and
 * returns `void`, mirroring `tokio::sync::mpsc::UnboundedSender::send`.
 *
 * For a **bounded** channel (`C` is a `number` capacity) `send` returns a
 * `Promise<void>` that resolves once the value has been accepted, applying
 * backpressure while the buffer is full — mirroring
 * `tokio::sync::mpsc::Sender::send`.
 */
export type MpscSendResult<C extends number | undefined> = C extends number
    ? Promise<void>
    : // biome-ignore lint/suspicious/noConfusingVoidType: unbounded send returns void by design
      void;

export class Mpsc<T, out C extends number | undefined = undefined> {
    public readonly sender: MpscSender<T, C>;
    public readonly receiver: MpscReceiver<T>;

    /**
     * Creates a channel.
     *
     * When `capacity` is a finite positive integer the channel is **bounded**:
     * once `capacity` buffered items are waiting to be received, further
     * `send`s apply backpressure — the returned promise only resolves once a
     * receiver frees up a slot. This mirrors the Rust
     * `tokio::sync::mpsc::channel(capacity)` semantics.
     *
     * When `capacity` is omitted (the default) the channel is **unbounded**:
     * `send` is synchronous and returns `void`, mirroring
     * `tokio::sync::mpsc::unbounded_channel()`.
     *
     * The `C` type parameter selects the `send` return type at the type level
     * (`void` for unbounded, `Promise<void>` for bounded). Because TypeScript
     * cannot infer it from the runtime `capacity` while `T` is given
     * explicitly, construct bounded channels as
     * `new Mpsc<T, number>(capacity)` and unbounded ones as `new Mpsc<T>()`.
     */
    constructor(capacity?: number) {
        const resolved =
            capacity === undefined || capacity === null
                ? Number.POSITIVE_INFINITY
                : capacity;

        if (
            Number.isFinite(resolved) &&
            (!Number.isInteger(resolved) || resolved < 1)
        ) {
            throw new RangeError(
                `Mpsc capacity must be a positive integer, got ${capacity}`,
            );
        }

        const state: MpscState<T> = {
            capacity: resolved,
            queue: [],
            waiters: [],
            sendWaiters: [],
            closed: false,
        };

        this.sender = new MpscSender(state);
        this.receiver = new MpscReceiver(state);
    }
}

type Waiter<T> = (value: IteratorResult<T>) => void;

type PendingSend<T> = {
    value: T;
    resolve: () => void;
    reject: (error: Error) => void;
};

type MpscState<T> = {
    // The maximum number of buffered items before `send` applies backpressure.
    // `Number.POSITIVE_INFINITY` means the channel is unbounded.
    readonly capacity: number;
    queue: T[];
    // a queue of functions registered to receive values, in the order they were awaited (FIFO)
    waiters: Waiter<T>[];
    // a queue of senders blocked waiting for buffer capacity (FIFO)
    sendWaiters: PendingSend<T>[];
    closed: boolean;
};

function isUnbounded<T>(state: MpscState<T>): boolean {
    return state.capacity === Number.POSITIVE_INFINITY;
}

/**
 * Whenever an item leaves the buffer, promote the next backpressured sender
 * (if any) into the freed slot and resolve its `send` promise.
 */
function promotePendingSend<T>(state: MpscState<T>): void {
    if (state.sendWaiters.length > 0 && state.queue.length < state.capacity) {
        // biome-ignore lint/style/noNonNullAssertion: length checked above
        const pending = state.sendWaiters.shift()!;
        state.queue.push(pending.value);
        pending.resolve();
    }
}

function closeState<T>(state: MpscState<T>): void {
    if (state.closed) {
        return;
    }

    state.closed = true;

    // Reject every sender still waiting for capacity; their values are dropped.
    while (state.sendWaiters.length > 0) {
        // biome-ignore lint/style/noNonNullAssertion: length checked above
        const pending = state.sendWaiters.shift()!;
        pending.reject(new MpscClosedError());
    }

    // Wake up all pending receivers to tell them it's over. Any already
    // buffered values remain drainable via `next()` because `next()` checks
    // the queue before the closed flag; receivers only register as waiters
    // when the queue is empty.
    while (state.waiters.length > 0) {
        // biome-ignore lint/style/noNonNullAssertion: length checked above
        const resolve = state.waiters.shift()!;
        resolve({ done: true, value: undefined });
    }
}

export class MpscReceiver<T> implements AsyncIterable<T> {
    constructor(private readonly state: MpscState<T>) {}

    /**
     * Receives the next value. If the queue is empty,
     * it returns a promise that resolves when a value is sent.
     */
    public next(): Promise<IteratorResult<T>> {
        // if there are values in the queue, return the first one
        if (this.state.queue.length > 0) {
            // biome-ignore lint/style/noNonNullAssertion: length checked above
            const value = this.state.queue.shift()!;
            // A slot just freed up — let a backpressured sender proceed.
            promotePendingSend(this.state);
            return Promise.resolve({
                done: false,
                value,
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
        closeState(this.state);
    }

    public isClosed(): boolean {
        return this.state.closed;
    }
}

export class MpscSender<T, out C extends number | undefined = undefined> {
    constructor(private readonly state: MpscState<T>) {}

    /**
     * Sends a value into the channel.
     *
     * On an unbounded channel this is synchronous (returns `void`) and throws
     * {@link MpscClosedError} if the channel is closed.
     *
     * On a bounded channel this returns a `Promise<void>` that resolves once
     * the value has been accepted — immediately while the buffer has spare
     * capacity, or after backpressure once a receiver frees a slot. The
     * promise rejects with {@link MpscClosedError} if the channel is (or
     * becomes) closed.
     */
    public send(value: T): MpscSendResult<C> {
        return this.dispatchSend(value) as MpscSendResult<C>;
    }

    private dispatchSend(value: T): void | Promise<void> {
        const unbounded = isUnbounded(this.state);

        if (this.state.closed) {
            if (unbounded) {
                throw new MpscClosedError();
            }
            return Promise.reject(new MpscClosedError());
        }

        // Hand the value straight to a waiting receiver.
        if (this.state.waiters.length > 0) {
            // biome-ignore lint/style/noNonNullAssertion: length checked above
            const resolve = this.state.waiters.shift()!;
            resolve({ done: false, value });
            return unbounded ? undefined : Promise.resolve();
        }

        // There is room in the buffer (always true for unbounded channels).
        if (this.state.queue.length < this.state.capacity) {
            this.state.queue.push(value);
            return unbounded ? undefined : Promise.resolve();
        }

        // Bounded buffer is full — apply backpressure until a slot frees up.
        return new Promise<void>((resolve, reject) => {
            this.state.sendWaiters.push({ value, resolve, reject });
        });
    }

    /**
     * Attempts to send a value without blocking.
     *
     * Returns `true` if the value was accepted, `false` if a bounded buffer is
     * currently full. Throws {@link MpscClosedError} if the channel is closed.
     */
    public trySend(value: T): boolean {
        if (this.state.closed) {
            throw new MpscClosedError();
        }

        if (this.state.waiters.length > 0) {
            // biome-ignore lint/style/noNonNullAssertion: length checked above
            const resolve = this.state.waiters.shift()!;
            resolve({ done: false, value });
            return true;
        }

        if (this.state.queue.length < this.state.capacity) {
            this.state.queue.push(value);
            return true;
        }

        return false;
    }

    public close() {
        closeState(this.state);
    }

    public isClosed(): boolean {
        return this.state.closed;
    }

    /**
     * Allows creating multiple producers from one channel
     */
    public clone(): MpscSender<T, C> {
        return new MpscSender(this.state);
    }
}

export class MpscClosedError extends Error {
    constructor(message?: string) {
        super(message ?? "Mpsc is closed");
        super.name = "MpscClosedError";
    }
}

export function mpsc<T, C extends number | undefined = undefined>(
    capacity?: number,
): Mpsc<T, C> {
    return new Mpsc<T, C>(capacity);
}
