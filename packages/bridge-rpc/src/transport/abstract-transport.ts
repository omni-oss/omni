import { Mutex } from "async-mutex";
import type { MaybePromise, Transport } from "./interface";
import { TransportReadFramer } from "./transport-read-framer";
import { TransportWriteFramer } from "./transport-write-framer";

export abstract class AbstractTransport implements Transport {
    protected onReceiveCallbacks: Array<
        (data: Uint8Array) => MaybePromise<void>
    > = [];
    private writeFramer = new TransportWriteFramer();
    private readFramer = new TransportReadFramer();
    private sendMutex = new Mutex();

    protected abstract sendBytes(data: Uint8Array): Promise<void>;

    protected receiveBytes = async (data: Uint8Array) => {
        const frames = this.readFramer.frame(data);

        if (frames && frames.length > 0) {
            for (const frame of frames) {
                for (const cb of this.onReceiveCallbacks) {
                    await cb(frame);
                }
            }
        }
    };

    send(data: Uint8Array): Promise<void> {
        return this.sendMutex.runExclusive(async () => {
            const [lengthPrefix, framedData] = this.writeFramer.frame(data);
            await this.sendBytes(lengthPrefix);
            await this.sendBytes(framedData);
        });
    }

    onReceive(callback: (data: Uint8Array) => void): void {
        this.onReceiveCallbacks.push(callback);
    }
}
