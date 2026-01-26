import type { Id } from "../../";
import type { Headers, Trailers } from "../dyn-map";
import type {
    ResponseBodyChunk,
    ResponseEnd,
    ResponseError,
    ResponseStart,
} from "../frame";

import type { ResponseStatusCode } from "../status-code";

// --- Enums and Unions ---

export enum ResponseState {
    INITIAL = "Initial",
    STARTED = "Started",
    BODY_CHUNKS_RECEIVING = "BodyChunksReceiving",
    ENDED = "Ended",
    ERRORED = "Errored",
}

export enum ResponseEventType {
    START = 0,
    BODY_CHUNK = 1,
    END = 2,
    ERROR = 3,
}

export type ResponseEvent =
    | { type: ResponseEventType.START; data: ResponseStart }
    | { type: ResponseEventType.BODY_CHUNK; data: ResponseBodyChunk }
    | { type: ResponseEventType.END; data: ResponseEnd }
    | { type: ResponseEventType.ERROR; data: ResponseError };

export type ResponseStateTransitionOutput =
    | { type: "Wait" }
    | {
          type: "Start";
          id: Id;
          status: ResponseStatusCode;
          headers?: Headers | undefined;
      }
    | { type: "BodyChunk"; chunk: Uint8Array }
    | { type: "End"; trailers?: Trailers | undefined }
    | { type: "Error"; error: ResponseError };

// --- Error Handling ---

export class ResponseStateMachineError extends Error {
    constructor(
        public readonly kind: string,
        message: string,
    ) {
        super(message);
        this.name = "ResponseStateMachineError";
    }
}

// --- State Machine ---

export class ResponseStateMachine {
    private _id: Id | null = null;
    private _state: ResponseState = ResponseState.INITIAL;

    public transition(event: ResponseEvent): ResponseStateTransitionOutput {
        const eventId = event.data.id;

        // ID Validation
        if (this._id !== null && !this._id.equals(eventId)) {
            throw new ResponseStateMachineError(
                "InvalidId",
                `Invalid id: expected ${this._id}, actual ${eventId}`,
            );
        }

        switch (this._state) {
            case ResponseState.INITIAL:
                if (event.type === ResponseEventType.START) {
                    this._state = ResponseState.STARTED;
                    this._id = event.data.id;
                    return {
                        type: "Start",
                        id: event.data.id,
                        status: event.data.status,
                        headers: event.data.headers ?? undefined,
                    };
                }
                throw this.invalidFrameError(
                    [ResponseEventType.START],
                    event.type,
                );

            case ResponseState.STARTED:
            case ResponseState.BODY_CHUNKS_RECEIVING:
                switch (event.type) {
                    case ResponseEventType.BODY_CHUNK:
                        this._state = ResponseState.BODY_CHUNKS_RECEIVING;
                        return {
                            type: "BodyChunk",
                            chunk: event.data.chunk,
                        };

                    case ResponseEventType.ERROR:
                        this._state = ResponseState.ERRORED;
                        return { type: "Error", error: event.data };

                    case ResponseEventType.END:
                        this._state = ResponseState.ENDED;
                        return {
                            type: "End",
                            trailers: event.data.trailers ?? undefined,
                        };

                    default:
                        throw this.invalidFrameError(
                            [ResponseEventType.START],
                            event.type,
                        );
                }

            case ResponseState.ENDED:
                throw new ResponseStateMachineError(
                    "Ended",
                    "cannot transition after received end frame",
                );

            case ResponseState.ERRORED:
                throw new ResponseStateMachineError(
                    "Errored",
                    "cannot transition after received error frame",
                );
        }
    }

    private invalidFrameError(
        expected: ResponseEventType[],
        actual: ResponseEventType,
    ): ResponseStateMachineError {
        return new ResponseStateMachineError(
            "InvalidFrame",
            `invalid frame type: expected: ${expected}, actual: ${actual}`,
        );
    }

    public get state(): ResponseState {
        return this._state;
    }
}
