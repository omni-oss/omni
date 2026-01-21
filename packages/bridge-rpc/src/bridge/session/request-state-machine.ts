import type { Id } from "../../";
import type { Headers, Trailers } from "../dyn-map";
import type {
    RequestBodyChunk,
    RequestEnd,
    RequestError,
    RequestStart,
} from "../frame";

export enum RequestState {
    INITIAL = "Initial",
    STARTED = "Started",
    BODY_CHUNKS_RECEIVING = "BodyChunksReceiving",
    ENDED = "Ended",
    ERRORED = "Errored",
}

export enum RequestEventType {
    START = 0,
    BODY_CHUNK = 1,
    END = 2,
    ERROR = 3,
}

export type RequestEvent =
    | { type: RequestEventType.START; data: RequestStart }
    | { type: RequestEventType.BODY_CHUNK; data: RequestBodyChunk }
    | { type: RequestEventType.END; data: RequestEnd }
    | { type: RequestEventType.ERROR; data: RequestError };

export type RequestStateTransitionOutput =
    | { type: "Wait" }
    | { type: "Start"; id: Id; path: string; headers?: Headers | undefined }
    | { type: "BodyChunk"; chunk: Uint8Array }
    | { type: "End"; trailers?: Trailers | undefined }
    | { type: "Error"; error: RequestError };

export class RequestStateMachineError extends Error {
    constructor(
        public readonly kind: string,
        message: string,
    ) {
        super(message);
        this.name = "RequestStateMachineError";
    }
}

export class RequestStateMachine {
    private _id: Id | null = null;
    private _state: RequestState = RequestState.INITIAL;

    public transition(event: RequestEvent): RequestStateTransitionOutput {
        const eventId = event.data.id;

        // ID Validation: Ensure all frames in this machine belong to the same Request ID
        if (this._id !== null && !this._id.equals(eventId)) {
            throw new RequestStateMachineError(
                "InvalidId",
                `invalid id: expected: ${this._id}, actual: ${eventId}`,
            );
        }

        switch (this._state) {
            case RequestState.INITIAL:
                if (event.type === RequestEventType.START) {
                    this._state = RequestState.STARTED;
                    this._id = event.data.id;
                    return {
                        type: "Start",
                        id: event.data.id,
                        path: event.data.path,
                        headers: event.data.headers ?? undefined,
                    };
                }
                throw this.invalidFrameError(
                    [RequestEventType.START],
                    event.type,
                );

            case RequestState.STARTED:
            case RequestState.BODY_CHUNKS_RECEIVING:
                switch (event.type) {
                    case RequestEventType.BODY_CHUNK:
                        if (this._state === RequestState.STARTED) {
                            this._state = RequestState.BODY_CHUNKS_RECEIVING;
                        }
                        return { type: "BodyChunk", chunk: event.data.chunk };

                    case RequestEventType.ERROR:
                        this._state = RequestState.ERRORED;
                        return { type: "Error", error: event.data };

                    case RequestEventType.END:
                        this._state = RequestState.ENDED;
                        return {
                            type: "End",
                            trailers: event.data.trailers ?? undefined,
                        };

                    default:
                        // Rust code expects BodyChunk in this branch for the error message
                        throw this.invalidFrameError(
                            [RequestEventType.BODY_CHUNK],
                            event.type,
                        );
                }

            case RequestState.ENDED:
                throw new RequestStateMachineError(
                    "Ended",
                    "cannot transition after received end frame",
                );

            case RequestState.ERRORED:
                throw new RequestStateMachineError(
                    "Errored",
                    "cannot transition after received error frame",
                );
        }
    }

    private invalidFrameError(
        expected: RequestEventType[],
        actual: RequestEventType,
    ): RequestStateMachineError {
        return new RequestStateMachineError(
            "InvalidFrame",
            `invalid frame type: expected: ${expected.join(", ")}, actual: ${actual}`,
        );
    }

    public get state(): RequestState {
        return this._state;
    }
}
