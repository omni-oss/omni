import type { MpscSender } from "@omni-oss/channels";
import type { Id } from "@/id";
import type { Headers, Trailers } from "../dyn-map";
import { Frame } from "../frame";
import type { PendingResponse } from "./response";

export type PendingResponseFactory = (
    id: Id,
) => Promise<PendingResponse> | PendingResponse;

export class PendingRequest {
    private _isStarted = false;

    constructor(
        private readonly id: Id,
        private readonly path: string,
        private readonly frameSender: MpscSender<Frame>,
        private readonly pendingResponseFactory: PendingResponseFactory,
    ) {}

    public async start(headers?: Headers | undefined) {
        if (this._isStarted) {
            throw new Error("Request already started");
        }

        this.frameSender.send(Frame.requestStart(this.id, this.path, headers));

        if (!this._isStarted) {
            this._isStarted = true;
        }

        return new ActiveRequest(
            this.id,
            this.frameSender,
            this.pendingResponseFactory,
        );
    }

    public get isStarted() {
        return this._isStarted;
    }
}

export class ActiveRequest {
    private _isEnded = false;
    constructor(
        private readonly id: Id,
        private readonly frameSender: MpscSender<Frame>,
        private readonly pendingResponseFactory: PendingResponseFactory,
    ) {}

    public async writeBodyChunk(chunk: Uint8Array) {
        await this.ensureNotEnded();
        this.frameSender.send(Frame.requestBodyChunk(this.id, chunk));
        return this;
    }

    public async end(trailers?: Trailers | undefined) {
        await this.ensureNotEnded();
        this.frameSender.send(Frame.requestEnd(this.id, trailers));
        this._isEnded = true;

        return await this.pendingResponseFactory(this.id);
    }

    public get isEnded() {
        return this._isEnded;
    }

    private async ensureNotEnded() {
        if (this._isEnded) {
            throw new Error("request is already ended");
        }
    }
}
