import type { Id } from "@/id";
import type { OneshotReceiver } from "@/oneshot";
import type { Headers, Trailers } from "../dyn-map";
import type { RequestError } from "../frame";

export class Request {
    private _trailers: Trailers | undefined;
    private isBodyRead = false;
    private isBodyReading = false;

    constructor(
        public readonly id: Id,
        public readonly path: string,
        public readonly headers: Headers | undefined,
        private readonly requestFrameEvents: AsyncIterable<RequestFrameEvent>,
        private readonly requestError: OneshotReceiver<RequestError>,
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

        for await (const event of this.requestFrameEvents) {
            if (this.requestError.hasValue()) {
                const value = await this.requestError.receive();

                throw new Error(
                    `Error from server: ${value.message}, error code: ${value.code}`,
                );
            }

            if (RequestFrameEvent.isBodyChunk(event)) {
                yield event.chunk;
            } else if (RequestFrameEvent.isEnd(event)) {
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

export enum RequestFrameEventType {
    BODY_CHUNK = 0,
    END = 1,
}

export type RequestFrameEvent =
    | {
          type: RequestFrameEventType.BODY_CHUNK;
          chunk: Uint8Array;
      }
    | {
          type: RequestFrameEventType.END;
          trailers?: Trailers | undefined;
      };

// 3. Helper Functions (Equivalent to 'new' and 'EnumIs')
export const RequestFrameEvent = {
    bodyChunk: (chunk: Uint8Array): RequestFrameEvent => ({
        type: RequestFrameEventType.BODY_CHUNK,
        chunk,
    }),
    end: (trailers?: Trailers): RequestFrameEvent => ({
        type: RequestFrameEventType.END,
        trailers,
    }),

    // Type guards (EnumIs equivalent)
    isBodyChunk: (
        event: RequestFrameEvent,
    ): event is Extract<
        RequestFrameEvent,
        { type: RequestFrameEventType.BODY_CHUNK }
    > => event.type === RequestFrameEventType.BODY_CHUNK,

    isEnd: (
        event: RequestFrameEvent,
    ): event is Extract<
        RequestFrameEvent,
        { type: RequestFrameEventType.END }
    > => event.type === RequestFrameEventType.END,
};
