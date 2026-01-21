import type { Id } from "@/id";
import type { MpscSender } from "@/mpsc";
import type { Headers } from "../dyn-map";
import { Frame } from "../frame";
import type { ResponseStatusCode } from "../status-code";

export class PendingResponse {
    constructor(
        private id: Id,
        private frameSender: MpscSender<Frame>,
    ) {}

    public async start(
        statusCode: ResponseStatusCode,
        headers?: Headers | undefined,
    ) {
        const frame = Frame.responseStart(this.id, statusCode, headers);
        this.frameSender.send(frame);
        return new ActiveResponse(this.id, this.frameSender);
    }
}

export class ActiveResponse {
    private isEnded = false;

    constructor(
        private id: Id,
        private frameSender: MpscSender<Frame>,
    ) {}

    public async writeBodyChunk(chunk: Uint8Array) {
        this.ensureNotEnded();
        const frame = Frame.responseBodyChunk(this.id, chunk);
        this.frameSender.send(frame);
    }

    public async end(trailers?: Headers | undefined) {
        this.ensureNotEnded();

        this.isEnded = true;
        const frame = Frame.responseEnd(this.id, trailers);
        this.frameSender.send(frame);
    }

    private async ensureNotEnded() {
        if (this.isEnded) {
            throw new Error("Response already ended");
        }
    }
}
