import z from "zod";
import { Id, IdConstructor } from "@/id";
import { HeadersSchema, TrailersSchema } from "./dyn-map";
import {
    RequestErrorCode,
    RequestErrorCodeConstructor,
    ResponseErrorCode,
    ResponseErrorCodeConstructor,
} from "./error-code";
import {
    ResponseStatusCode,
    ResponseStatusCodeConstructor,
} from "./status-code";

// --- External Type Placeholders ---
// Replace these with your actual imported schemas
const IdSchema = z
    .union([z.bigint(), z.instanceof(IdConstructor)])
    .transform((v) => {
        if (v instanceof Id) {
            return v;
        }
        return Id.fromBigInt(v);
    });
const RequestErrorCodeSchema = z
    .union([z.number(), z.instanceof(RequestErrorCodeConstructor)])
    .transform((v) => {
        if (v instanceof RequestErrorCode) {
            return v;
        }
        return RequestErrorCode.from(v);
    });
const ResponseErrorCodeSchema = z
    .union([z.number(), z.instanceof(ResponseErrorCodeConstructor)])
    .transform((v) => {
        if (v instanceof ResponseErrorCode) {
            return v;
        }
        return ResponseErrorCode.from(v);
    });
const ResponseStatusCodeSchema = z
    .union([z.number(), z.instanceof(ResponseStatusCodeConstructor)])
    .transform((v) => {
        if (v instanceof ResponseStatusCode) {
            return v;
        }
        return ResponseStatusCode.from(v);
    });

export enum FrameType {
    REQUEST_START = 0,
    REQUEST_BODY_CHUNK = 1,
    REQUEST_END = 2,
    REQUEST_ERROR = 3,

    RESPONSE_START = 20,
    RESPONSE_BODY_CHUNK = 21,
    RESPONSE_END = 22,
    RESPONSE_ERROR = 23,

    CLOSE = 40,
    PING = 41,
    PONG = 42,
}

export const FrameTypeSchema = z.enum(FrameType);

// --- Data Structure Schemas ---

export const RequestStartSchema = z.object({
    id: IdSchema,
    path: z.string(),
    headers: z.nullish(HeadersSchema),
});

export const RequestBodyChunkSchema = z.object({
    id: IdSchema,
    chunk: z.instanceof(Uint8Array),
});

export const RequestEndSchema = z.object({
    id: IdSchema,
    trailers: z.nullish(TrailersSchema),
});

export const RequestErrorSchema = z.object({
    id: IdSchema,
    code: RequestErrorCodeSchema,
    message: z.string(),
});

export const ResponseStartSchema = z.object({
    id: IdSchema,
    status: ResponseStatusCodeSchema,
    headers: z.nullish(HeadersSchema),
});

export const ResponseBodyChunkSchema = z.object({
    id: IdSchema,
    chunk: z.instanceof(Uint8Array),
});

export const ResponseEndSchema = z.object({
    id: IdSchema,
    trailers: z.nullish(TrailersSchema),
});

export const ResponseErrorSchema = z.object({
    id: IdSchema,
    code: ResponseErrorCodeSchema,
    message: z.string(),
});

// --- Main Frame Discriminated Union ---

export const FrameSchema = z.discriminatedUnion("type", [
    z.object({
        type: z.literal(FrameType.REQUEST_START),
        data: RequestStartSchema,
    }),
    z.object({
        type: z.literal(FrameType.REQUEST_BODY_CHUNK),
        data: RequestBodyChunkSchema,
    }),
    z.object({
        type: z.literal(FrameType.REQUEST_END),
        data: RequestEndSchema,
    }),
    z.object({
        type: z.literal(FrameType.REQUEST_ERROR),
        data: RequestErrorSchema,
    }),

    z.object({
        type: z.literal(FrameType.RESPONSE_START),
        data: ResponseStartSchema,
    }),
    z.object({
        type: z.literal(FrameType.RESPONSE_BODY_CHUNK),
        data: ResponseBodyChunkSchema,
    }),
    z.object({
        type: z.literal(FrameType.RESPONSE_END),
        data: ResponseEndSchema,
    }),
    z.object({
        type: z.literal(FrameType.RESPONSE_ERROR),
        data: ResponseErrorSchema,
    }),

    z.object({ type: z.literal(FrameType.CLOSE), data: z.null() }), // CLOSE
    z.object({ type: z.literal(FrameType.PING), data: z.null() }), // PING
    z.object({ type: z.literal(FrameType.PONG), data: z.null() }), // PONG
]);
