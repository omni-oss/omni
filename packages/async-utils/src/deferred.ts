export enum DeferredState {
    PENDING = "PENDING",
    RESOLVED = "RESOLVED",
    REJECTED = "REJECTED",
}

export class Deferred<T> {
    private _reject!: (reason?: unknown) => void;
    private _resolve!: (value: T) => void;
    private _state: DeferredState = DeferredState.PENDING;
    private readonly _promise: Promise<T>;

    constructor() {
        this._promise = new Promise<T>((resolve, reject) => {
            this._resolve = resolve;
            this._reject = reject;
        });
    }

    get promise() {
        return this._promise;
    }

    resolve(value: T) {
        if (this._state !== DeferredState.PENDING) {
            throw new Error(
                "cannot resolve a Deferred that is not in pending state",
            );
        }
        this._state = DeferredState.RESOLVED;
        this._resolve(value);
    }

    reject(reason?: unknown) {
        if (this._state !== DeferredState.PENDING) {
            throw new Error(
                "cannot reject a Deferred that is not in pending state",
            );
        }
        this._state = DeferredState.REJECTED;
        this._reject(reason);
    }

    get state() {
        return this._state;
    }
}

export function createDeferred<T>(): Deferred<T> {
    return new Deferred();
}
