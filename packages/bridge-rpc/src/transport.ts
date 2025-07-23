export interface Transport {
    send(data: Uint8Array): Promise<void>;
    onReceive(callback: (data: Uint8Array) => void): void;
}
