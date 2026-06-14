import type { OneshotReceiver } from "@omni-oss/channels";
import type { Id } from "@/id";
import type { Headers, Trailers } from "../dyn-map";
import { throwIfError } from "../error-utils";
import type { RequestError as RequestErrorFrame } from "../frame";

export class Request {
    private _trailers: Trailers | undefined;
    private _isBodyRead = false;
    private _isBodyReading = false;

    constructor(
        public readonly id: Id,
        public readonly path: string,
        public readonly headers: Headers | undefined,
        private readonly requestFrameEvents: AsyncIterable<RequestFrameEvent>,
        private readonly requestError: OneshotReceiver<RequestErrorFrame>,
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

        // Surface an error that arrived before any body frame.
        await this.throwIfError();

        for await (const event of this.requestFrameEvents) {
            await this.throwIfError();

            if (RequestFrameEvent.isBodyChunk(event)) {
                yield event.chunk;
            } else if (RequestFrameEvent.isEnd(event)) {
                this._trailers = event.trailers;
                this._isBodyRead = true;
                this._isBodyReading = false;
                return;
            } else {
                throw new Error("Invalid RequestFrameEvent");
            }
        }

        // The frame channel closed without an explicit end frame. This
        // happens when the session is torn down (for example because the
        // client sent a request error frame). Surface the error if one was
        // delivered so the handler fails instead of silently terminating.
        await this.throwIfError();
    }

    private throwIfError() {
        return throwIfError(this.requestError);
    }

    async [Symbol.asyncDispose]() {
        // If the body has not been fully read, we need to consume the remaining body to ensure proper disposal
        if (!this._isBodyRead) {
            this._isBodyReading = false;
            for await (const _ of this.readBody()) {
                // Consume the body to ensure proper disposal
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
