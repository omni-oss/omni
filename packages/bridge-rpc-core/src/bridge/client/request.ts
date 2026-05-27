import type { MpscSender } from "@omni-oss/channels";
import type { Id } from "@/id";
import type { Headers, Trailers } from "../dyn-map";
import { Frame } from "../frame";
import type { PendingResponse } from "./response";

export type PendingResponseFactory = (
    id: Id,
) => Promise<PendingResponse> | PendingResponse;

export class PendingRequest {
    constructor(
        private readonly id: Id,
        private readonly path: string,
        private readonly frameSender: MpscSender<Frame>,
        private readonly pendingResponseFactory: PendingResponseFactory,
    ) {}

    public async start(headers?: Headers | undefined) {
        this.frameSender.send(Frame.requestStart(this.id, this.path, headers));

        return new ActiveRequest(
            this.id,
            this.frameSender,
            this.pendingResponseFactory,
        );
    }
}

export class ActiveRequest {
    private isEnded = false;
    constructor(
        private readonly id: Id,
        private readonly frameSender: MpscSender<Frame>,
        private readonly pendingResponseFactory: PendingResponseFactory,
    ) {}

    public async writeBodyChunk(chunk: Uint8Array) {
        this.ensureNotEnded();
        this.frameSender.send(Frame.requestBodyChunk(this.id, chunk));
    }

    public async end(trailers?: Trailers | undefined) {
        this.ensureNotEnded();
        this.frameSender.send(Frame.requestEnd(this.id, trailers));

        return await this.pendingResponseFactory(this.id);
    }

    private ensureNotEnded() {
        if (this.isEnded) {
            throw new Error("request is already ended");
        }
    }
}
