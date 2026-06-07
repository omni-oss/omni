import { bindAsyncContext } from "@omni-oss/async-utils";
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

    onReceive(callback: (data: Uint8Array) => MaybePromise<void>): void {
        // Snapshot the async context at registration time so that the
        // callback always observes the `AsyncLocalStorage` stores that
        // were active when it was registered — even when the underlying
        // stream pump (e.g. `pipeTo` wired up in a transport's
        // constructor) was set up *outside* any `als.run(...)` scope and
        // would otherwise invoke us in the empty (root) async context.
        //
        // Without this, callbacks registered from inside e.g.
        // `withLogTapeRoot(...)` lose the ambient logger store as soon
        // as the stream emits a chunk, because the chunk's microtask
        // chain traces back to the transport's construction site, not
        // to the registration site.
        this.onReceiveCallbacks.push(bindAsyncContext(callback));
    }
}
