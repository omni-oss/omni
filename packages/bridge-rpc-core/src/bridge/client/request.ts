import type { MpscSender, OneshotReceiver } from "@omni-oss/channels";
import type { Id } from "@/id";
import type { Headers, Trailers } from "../dyn-map";
import type { RequestErrorCode } from "../error-code";
import { throwIfError } from "../error-utils";
import { Frame, type ResponseError as ResponseErrorFrame } from "../frame";
import type { PendingResponse } from "./response";

export type PendingResponseFactory = (
    id: Id,
) => Promise<PendingResponse> | PendingResponse;

export class PendingRequest {
    private _isStarted = false;

    constructor(
        private readonly id: Id,
        private readonly path: string,
        private readonly frameSender: MpscSender<Frame, number | undefined>,
        private readonly errorReceiver: OneshotReceiver<ResponseErrorFrame>,
        private readonly pendingResponseFactory: PendingResponseFactory,
    ) {}

    public async start(headers?: Headers | undefined) {
        if (this._isStarted) {
            throw new Error("Request already started");
        }

        await this.ensureNotError();

        await this.frameSender.send(
            Frame.requestStart(this.id, this.path, headers),
        );

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

    private ensureNotError() {
        return throwIfError(this.errorReceiver);
    }
}

export class ActiveRequest {
    private _isEnded = false;
    constructor(
        private readonly id: Id,
        private readonly frameSender: MpscSender<Frame, number | undefined>,
        private readonly errorReceiver: OneshotReceiver<ResponseErrorFrame>,
        private readonly pendingResponseFactory: PendingResponseFactory,
    ) {}

    public async writeBodyChunk(chunk: Uint8Array) {
        await this.ensureNotEnded();
        await this.ensureNotError();
        await this.frameSender.send(Frame.requestBodyChunk(this.id, chunk));
        return this;
    }

    public async end(trailers?: Trailers | undefined) {
        await this.ensureNotEnded();
        await this.ensureNotError();
        await this.frameSender.send(Frame.requestEnd(this.id, trailers));
        this._isEnded = true;

        return await this.pendingResponseFactory(this.id);
    }

    /**
     * Aborts the in-flight request by sending a request error frame to the
     * server, mirroring the Rust `ActiveRequest::error`. After this the
     * request is considered ended and no further frames may be sent.
     */
    public async error(code: RequestErrorCode, message: string) {
        await this.ensureNotEnded();
        await this.ensureNotError();
        await this.frameSender.send(Frame.requestError(this.id, code, message));
        this._isEnded = true;
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
    private ensureNotError() {
        return throwIfError(this.errorReceiver);
    }
}
