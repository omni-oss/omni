import { Mpsc, type MpscReceiver } from "@omni-oss/channels";
import { Mutex } from "async-mutex";
import { encode } from "./codec-utils";
import type { Frame } from "./frame";

type SendBytesFn = (chunk: Uint8Array) => Promise<void>;

export class FrameTransporter {
    private task: { promise: Promise<void> } | undefined;
    private mpsc = new Mpsc<Frame>();
    private mutex = new Mutex();

    constructor(private sendBytesFn: SendBytesFn) {}

    public get sender() {
        if (!this.isRunning) {
            throw new Error("Worker is not running");
        }

        return this.mpsc.sender;
    }

    public get isRunning() {
        return this.task !== undefined;
    }

    public start() {
        return this.runExclusive(() => {
            if (this.task !== undefined) {
                throw new Error("FrameTransporter already started");
            }

            this.task = {
                promise: this.run(this.sendBytesFn, this.mpsc.receiver),
            };
        });
    }

    public async stop() {
        return await this.runExclusive(async () => {
            if (this.task === undefined) {
                throw new Error("FrameTransporter not started");
            }

            this.mpsc.sender.close();

            await this.task.promise;

            this.task = undefined;
            this.mpsc = new Mpsc<Frame>();
        });
    }

    private async run(
        sendBytesFn: SendBytesFn,
        frameReceiver: MpscReceiver<Frame>,
    ) {
        for await (const bytes of frameReceiver) {
            await sendBytesFn(encode(bytes));
        }
    }

    private runExclusive<T>(fn: () => T): Promise<T> {
        return this.mutex.runExclusive(fn);
    }
}
