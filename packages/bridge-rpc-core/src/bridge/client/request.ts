import type { MpscSender, OneshotReceiver } from "@omni-oss/channels";
import type { Id } from "@/id";
import type { Headers, Trailers } from "../dyn-map";
import { Frame, type ResponseError } from "../frame";
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
        private readonly errorReceiver: OneshotReceiver<ResponseError>,
        private readonly pendingResponseFactory: PendingResponseFactory,
    ) {}

    public async start(headers?: Headers | undefined) {
        if (this._isStarted) {
            throw new Error("Request already started");
        }

        await this.ensureNotError();

        this.frameSender.send(Frame.requestStart(this.id, this.path, headers));

        await this.ensureNotError();

        if (!this._isStarted) {
            this._isStarted = true;
        }

        await this.ensureNotError();

        return new ActiveRequest(
            this.id,
            this.frameSender,
            this.errorReceiver,
            this.pendingResponseFactory,
        );
    }

    public get isStarted() {
        return this._isStarted;
    }

    private async ensureNotError() {
        if (this.errorReceiver.hasValue()) {
            const error = await this.errorReceiver.receive();
            throw new Error(
                `Request failed with error code ${error.code.toString()}, ${error.message}`,
                {
                    cause: error,
                },
            );
        }
    }
}

export class ActiveRequest {
    private _isEnded = false;
    constructor(
        private readonly id: Id,
        private readonly frameSender: MpscSender<Frame>,
        private readonly errorReceiver: OneshotReceiver<ResponseError>,
        private readonly pendingResponseFactory: PendingResponseFactory,
    ) {}

    public async writeBodyChunk(chunk: Uint8Array) {
        await this.ensureNotEnded();
        await this.ensureNotError();
        this.frameSender.send(Frame.requestBodyChunk(this.id, chunk));
        return this;
    }

    public async end(trailers?: Trailers | undefined) {
        await this.ensureNotEnded();
        await this.ensureNotError();
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

    async [Symbol.asyncDispose]() {
        if (!this._isEnded) {
            await this.end();
        }
    }
    private async ensureNotError() {
        if (this.errorReceiver.hasValue()) {
            const error = await this.errorReceiver.receive();
            this._isEnded = true; // prevent further sends on this request
            throw new Error(
                `Request failed with error code ${error.code.toString()}, ${error.message}`,
                {
                    cause: error,
                },
            );
        }
    }
}
