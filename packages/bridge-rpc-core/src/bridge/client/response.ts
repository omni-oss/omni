import type { OneshotReceiver } from "@omni-oss/channels";
import type { Id } from "@/id";
import type { Headers, Trailers } from "../dyn-map";
import type { ResponseError, ResponseStart } from "../frame";
import type { ResponseStatusCode } from "../status-code";

export class PendingResponse {
    constructor(
        private readonly id: Id,
        private readonly responseStartReceiver: OneshotReceiver<ResponseStart>,
        private readonly responseFrameReceiver: AsyncIterable<ResponseFrameEvent>,
        private readonly responseErrorReceiver: OneshotReceiver<ResponseError>,
    ) {}

    public async wait() {
        const responseStart = await this.responseStartReceiver.receive();

        return new Response(
            this.id,
            responseStart.status,
            responseStart.headers ?? undefined,
            this.responseFrameReceiver,
            this.responseErrorReceiver,
        );
    }
}

export class Response {
    private isBodyRead = false;
    private isBodyReading = false;
    private _trailers?: Trailers | undefined;

    constructor(
        public readonly id: Id,
        public readonly status: ResponseStatusCode,
        public readonly headers: Headers | undefined,
        private readonly responseFrameReceiver: AsyncIterable<ResponseFrameEvent>,
        private readonly responseErrorReceiver: OneshotReceiver<ResponseError>,
    ) {}

    public get trailers(): Trailers | undefined {
        if (!this.isBodyRead) {
            throw new Error("Body has not been read");
        }

        if (this.isBodyReading) {
            throw new Error(
                "Body is being read, cannot access trailers until read is complete",
            );
        }

        return this._trailers;
    }

    async *readBody(): AsyncIterable<Uint8Array> {
        if (this.isBodyRead) {
            throw new Error("Body has already been read");
        }

        if (this.isBodyReading) {
            throw new Error("Body is already being read");
        }

        this.isBodyReading = true;

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
                this.isBodyRead = true;
                this.isBodyReading = false;
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
