import type { OneshotReceiver } from "@omni-oss/channels";
import type { Id } from "@/id";
import type { Headers, Trailers } from "../dyn-map";
import type { ResponseError, ResponseStart } from "../frame";
import type { ResponseStatusCode } from "../status-code";

export class PendingResponse {
    private _isStarted = false;

    constructor(
        private readonly id: Id,
        private readonly responseStartReceiver: OneshotReceiver<ResponseStart>,
        private readonly responseFrameReceiver: AsyncIterable<ResponseFrameEvent>,
        private readonly responseErrorReceiver: OneshotReceiver<ResponseError>,
    ) {}

    public async wait() {
        if (this._isStarted) {
            throw new Error("Response already started");
        }

        const responseStart = await this.responseStartReceiver.receive();

        this._isStarted = true;

        return new Response(
            this.id,
            responseStart.status,
            responseStart.headers ?? undefined,
            this.responseFrameReceiver,
            this.responseErrorReceiver,
        );
    }

    public get isStarted() {
        return this._isStarted;
    }
}

export class Response {
    private _isBodyRead = false;
    private _isBodyReading = false;
    private _trailers?: Trailers | undefined;

    constructor(
        public readonly id: Id,
        public readonly status: ResponseStatusCode,
        public readonly headers: Headers | undefined,
        private readonly responseFrameReceiver: AsyncIterable<ResponseFrameEvent>,
        private readonly responseErrorReceiver: OneshotReceiver<ResponseError>,
    ) {}

    public get trailers(): Trailers | undefined {
        if (!this._isBodyRead) {
            throw new Error("Body has not been read");
        }

        if (this._isBodyReading) {
            throw new Error(
                "Body is being read, cannot access trailers until read is complete",
            );
        }

        return this._trailers;
    }

    async *readBody(): AsyncIterable<Uint8Array> {
        if (this._isBodyRead) {
            throw new Error("Body has already been read");
        }

        if (this._isBodyReading) {
            throw new Error("Body is already being read");
        }

        this._isBodyReading = true;

        for await (const event of this.responseFrameReceiver) {
            if (this.responseErrorReceiver.hasValue()) {
                const value = await this.responseErrorReceiver.receive();

                throw new Error(
                    `Error from server: ${value.message}, error code: ${value.code}`,
                );
            }

            if (ResponseFrameEvent.isBodyChunk(event)) {
                yield event.chunk;
            } else if (ResponseFrameEvent.isEnd(event)) {
                this._trailers = event.trailers;
                this._isBodyRead = true;
                this._isBodyReading = false;
                break;
            } else {
                throw new Error("Invalid RequestFrameEvent");
            }
        }
    }
}

export enum ResponseFrameEventType {
    BODY_CHUNK = 0,
    END = 1,
}

export type ResponseFrameEvent =
    | {
          type: ResponseFrameEventType.BODY_CHUNK;
          chunk: Uint8Array;
      }
    | {
          type: ResponseFrameEventType.END;
          trailers?: Trailers | undefined;
      };

export const ResponseFrameEvent = {
    bodyChunk: (chunk: Uint8Array): ResponseFrameEvent => ({
        type: ResponseFrameEventType.BODY_CHUNK,
        chunk,
    }),
    end: (trailers?: Trailers): ResponseFrameEvent => ({
        type: ResponseFrameEventType.END,
        trailers,
    }),

    // Type guards (EnumIs equivalent)
    isBodyChunk: (
        event: ResponseFrameEvent,
    ): event is Extract<
        ResponseFrameEvent,
        { type: ResponseFrameEventType.BODY_CHUNK }
    > => event.type === ResponseFrameEventType.BODY_CHUNK,

    isEnd: (
        event: ResponseFrameEvent,
    ): event is Extract<
        ResponseFrameEvent,
        { type: ResponseFrameEventType.END }
    > => event.type === ResponseFrameEventType.END,
};
