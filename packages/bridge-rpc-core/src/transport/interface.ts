export interface Transport {
    send(data: Uint8Array): Promise<void>;
    onReceive(callback: (data: Uint8Array) => MaybePromise<void>): void;
}

export type MaybePromise<T> = T | Promise<T>;
