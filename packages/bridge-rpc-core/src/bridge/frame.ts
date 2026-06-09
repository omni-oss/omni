import type z from "zod";
import type { Id } from "..";
import type { Headers, Trailers } from "./dyn-map";
import type { RequestErrorCode, ResponseErrorCode } from "./error-code";
import {
    type FrameSchema,
    FrameType,
    RequestBodyChunkSchema,
    RequestEndSchema,
    RequestErrorSchema,
    RequestStartSchema,
    ResponseBodyChunkSchema,
    ResponseEndSchema,
    ResponseErrorSchema,
    ResponseStartSchema,
} from "./frame-schema";
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

    fromTuple: (tuple: [FrameType, unknown]): Frame => {
        const [type, data] = tuple;
        switch (type) {
            case FrameType.REQUEST_START:
                return { type, data: RequestStartSchema.parse(data) };
            case FrameType.REQUEST_BODY_CHUNK:
                return { type, data: RequestBodyChunkSchema.parse(data) };
            case FrameType.REQUEST_END:
                return { type, data: RequestEndSchema.parse(data) };
            case FrameType.REQUEST_ERROR:
                return { type, data: RequestErrorSchema.parse(data) };
            case FrameType.RESPONSE_START:
                return { type, data: ResponseStartSchema.parse(data) };
            case FrameType.RESPONSE_BODY_CHUNK:
                return { type, data: ResponseBodyChunkSchema.parse(data) };
            case FrameType.RESPONSE_END:
                return { type, data: ResponseEndSchema.parse(data) };
            case FrameType.RESPONSE_ERROR:
                return { type, data: ResponseErrorSchema.parse(data) };
            case FrameType.PING:
            case FrameType.PONG:
            case FrameType.CLOSE:
                return { type, data: null };
            default:
                throw new Error(`Unknown frame type: ${type}`);
        }
    },

    toTuple: (frame: Frame): [FrameType, unknown] => {
        switch (frame.type) {
            case FrameType.REQUEST_START:
            case FrameType.REQUEST_BODY_CHUNK:
            case FrameType.REQUEST_END:
            case FrameType.REQUEST_ERROR:
            case FrameType.RESPONSE_START:
            case FrameType.RESPONSE_BODY_CHUNK:
            case FrameType.RESPONSE_END:
            case FrameType.RESPONSE_ERROR:
                return [frame.type, frame.data];
            case FrameType.PING:
            case FrameType.PONG:
            case FrameType.CLOSE:
                return [frame.type, null];
            default:
                throw new Error(
                    `Unknown frame type: ${(frame as { type: number }).type}`,
                );
        }
    },
};
