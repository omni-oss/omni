import type z from "zod";
import type { Id } from "..";
import type { Headers, Trailers } from "./dyn-map";
import type { RequestErrorCode, ResponseErrorCode } from "./error-code";
import type {
    FrameSchema,
    RequestBodyChunkSchema,
    RequestEndSchema,
    RequestErrorSchema,
    RequestStartSchema,
    ResponseBodyChunkSchema,
    ResponseEndSchema,
    ResponseErrorSchema,
    ResponseStartSchema,
} from "./frame-schema";
import { FrameType } from "./frame-schema";
import type { ResponseStatusCode } from "./status-code";

export { FrameType } from "./frame-schema";

// --- Data Structures ---

export type RequestStart = z.infer<typeof RequestStartSchema>;

export type RequestBodyChunk = z.infer<typeof RequestBodyChunkSchema>;

export type RequestEnd = z.infer<typeof RequestEndSchema>;

export type RequestError = z.infer<typeof RequestErrorSchema>;

export type ResponseStart = z.infer<typeof ResponseStartSchema>;
export type ResponseBodyChunk = z.infer<typeof ResponseBodyChunkSchema>;

export type ResponseEnd = z.infer<typeof ResponseEndSchema>;

export type ResponseError = z.infer<typeof ResponseErrorSchema>;

export type Frame = z.infer<typeof FrameSchema>;

/**
 * Equivalent to the Frame implementation block in Rust
 */
export const Frame = {
    requestStart: (id: Id, path: string, headers?: Headers): Frame => ({
        type: FrameType.REQUEST_START,
        data: { id, path, headers },
    }),

    requestBodyChunk: (id: Id, chunk: Uint8Array): Frame => ({
        type: FrameType.REQUEST_BODY_CHUNK,
        data: { id, chunk: chunk as Uint8Array<ArrayBuffer> },
    }),

    requestEnd: (id: Id, trailers?: Trailers): Frame => ({
        type: FrameType.REQUEST_END,
        data: { id, trailers },
    }),

    requestError: (id: Id, code: RequestErrorCode, message: string): Frame => ({
        type: FrameType.REQUEST_ERROR,
        data: { id, code, message },
    }),

    responseStart: (
        id: Id,
        status: ResponseStatusCode,
        headers?: Headers,
    ): Frame => ({
        type: FrameType.RESPONSE_START,
        data: { id, status, headers },
    }),

    responseBodyChunk: (id: Id, chunk: Uint8Array): Frame => ({
        type: FrameType.RESPONSE_BODY_CHUNK,
        data: { id, chunk: chunk as Uint8Array<ArrayBuffer> },
    }),

    responseEnd: (id: Id, trailers?: Trailers): Frame => ({
        type: FrameType.RESPONSE_END,
        data: { id, trailers },
    }),

    responseError: (
        id: Id,
        code: ResponseErrorCode,
        message: string,
    ): Frame => ({
        type: FrameType.RESPONSE_ERROR,
        data: { id, code, message },
    }),

    ping: (): Frame => ({ type: FrameType.PING, data: null }),
    pong: (): Frame => ({ type: FrameType.PONG, data: null }),
    close: (): Frame => ({ type: FrameType.CLOSE, data: null }),
};
