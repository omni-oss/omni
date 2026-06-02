import type { MpscSender } from "@omni-oss/channels";
import type { Id } from "@/id";
import type { Headers } from "../dyn-map";
import { Frame } from "../frame";
import type { ResponseStatusCode } from "../status-code";

export class PendingResponse {
    private _isStarted = false;

    constructor(
        private id: Id,
        private frameSender: MpscSender<Frame>,
    ) {}

    public async start(
        statusCode: ResponseStatusCode,
        headers?: Headers | undefined,
    ) {
        if (this._isStarted) {
            throw new Error("Response already started");
        }

        const frame = Frame.responseStart(this.id, statusCode, headers);
        this.frameSender.send(frame);
        this._isStarted = true;
        return new ActiveResponse(this.id, this.frameSender);
    }

    public get isStarted() {
        return this._isStarted;
    }
}

export class ActiveResponse {
    private _isEnded = false;

    constructor(
        private id: Id,
        private frameSender: MpscSender<Frame>,
    ) {}

    public async writeBodyChunk(chunk: Uint8Array) {
        await this.ensureNotEnded();
        const frame = Frame.responseBodyChunk(this.id, chunk);
        this.frameSender.send(frame);
    }

    public async end(trailers?: Headers | undefined) {
        await this.ensureNotEnded();
        this._isEnded = true;

        const frame = Frame.responseEnd(this.id, trailers);
        this.frameSender.send(frame);
    }

    public get isEnded() {
        return this._isEnded;
    }

    private async ensureNotEnded() {
        if (this._isEnded) {
            throw new Error("Response already ended");
        }
    }
}
