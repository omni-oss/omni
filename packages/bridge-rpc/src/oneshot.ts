export class Oneshot<T> {
    public readonly sender: OneshotSender<T>;
    public readonly receiver: OneshotReceiver<T>;

    constructor() {
        const state = {
            sent: false,
            closed: false,
        };
        let closerFn!: () => void;
        let senderFn!: (vaue: T) => void;

        const promise = new Promise<T>((resolve, reject) => {
            senderFn = (t) => {
                if (state.closed) {
                    throw new OneshotClosedError();
                }

                if (state.sent) {
                    throw new OneshotValueSentError();
                }

                state.sent = true;
                resolve(t);
            };
            closerFn = () => {
                if (state.closed) {
                    return;
                }

                state.closed = true;
                if (!state.sent) {
                    return reject(new OneshotClosedError());
                }
            };
        });

        this.sender = new OneshotSender(senderFn, closerFn, state);
        this.receiver = new OneshotReceiver(promise, closerFn, state);
    }
}

type OneshotState = {
    readonly closed: boolean;
    readonly sent: boolean;
};

export class OneshotReceiver<T> {
    constructor(
        private readonly promise: Promise<T>,
        private readonly closerFn: () => void,
        private readonly state: OneshotState,
    ) {}

    public receive(): Promise<T> {
        return this.promise;
    }

    public hasValue(): boolean {
        return this.state.sent;
    }

    public isClosed(): boolean {
        return this.state.closed;
    }

    public close() {
        if (this.state.closed) {
            throw new OneshotClosedError();
        }

        this.closerFn();
    }
}

export class OneshotSender<T> {
    constructor(
        private readonly senderFn: (value: T) => void,
        private readonly closerFn: () => void,
        private readonly state: OneshotState,
    ) {}

    public isClosed() {
        return this.state.closed;
    }

    public isSent() {
        return this.state.sent;
    }

    public send(value: T) {
        if (this.state.closed) {
            throw new OneshotClosedError();
        }

        if (this.state.sent) {
            throw new OneshotValueSentError();
        }

        this.senderFn(value);
    }

    public close() {
        if (this.state.closed) {
            throw new OneshotClosedError();
        }

        this.closerFn();
    }
}

export class OneshotClosedError extends Error {
    constructor() {
        super("Oneshot is closed");
        this.name = "OneshotClosedError";
    }
}

export class OneshotValueSentError extends Error {
    constructor() {
        super("Oneshot value has already been sent");
        this.name = "OneshotValueSentError";
    }
}

export function createOneshot<T>(): Oneshot<T> {
    return new Oneshot<T>();
}
