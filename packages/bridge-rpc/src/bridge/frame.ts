import * as z from "zod";
import { type Id, IdSchema } from "./id";

export enum FrameType {
    CLOSE = 0,
    CLOSE_ACK = 1,
    PROBE = 2,
    PROBE_ACK = 3,
    STREAM_START = 4,
    STREAM_START_RESPONSE = 5,
    STREAM_DATA = 6,
    STREAM_END = 7,
    MESSAGE_REQUEST = 8,
    MESSAGE_RESPONSE = 9,
}

// Frame Types enum
export const FrameTypeSchema = z.enum(FrameType);

// Error Data
export const ErrorDataSchema = z.object({
    message: z.string(),
});
export type ErrorData = z.infer<typeof ErrorDataSchema>;

// Stream Start
export function StreamStartSchema<TData extends z.ZodType>(schema: TData) {
    return z.object({
        id: IdSchema,
        path: z.string(),
        data: z.nullable(schema),
    });
}
export const UnknownStreamStartSchema = StreamStartSchema(z.unknown());
export type UnknownStreamStart = z.infer<typeof UnknownStreamStartSchema>;

// Stream Start Response
export const StreamStartResponseSchema = z.object({
    id: IdSchema,
    ok: z.boolean(),
    error: z.nullish(ErrorDataSchema),
});
export type StreamStartResponse = z.infer<typeof StreamStartResponseSchema>;

// Stream Data
export function StreamDataSchema<TData extends z.ZodType>(dataSchema: TData) {
    return z.object({
        id: IdSchema,
        data: dataSchema,
    });
}
export const UnknownStreamDataSchema = StreamDataSchema(z.unknown());
export type UnknownStreamData = z.infer<typeof UnknownStreamDataSchema>;

// Stream End
export const StreamEndSchema = z.object({
    id: IdSchema,
    error: z.nullish(ErrorDataSchema),
});
export type StreamEnd = z.infer<typeof StreamEndSchema>;

// Request
export function RequestSchema<TData extends z.ZodType>(schema: TData) {
    return z.object({
        id: IdSchema,
        path: z.string(),
        data: schema,
    });
}
export const UnknownRequestSchema = RequestSchema(z.unknown());
export type UnknownRequest = z.infer<typeof UnknownRequestSchema>;

// Response
export function ResponseSchema<TData extends z.ZodType>(schema: TData) {
    return z.object({
        id: IdSchema,
        data: z.nullish(schema),
        error: z.nullish(ErrorDataSchema),
    });
}
export type Response<TData extends z.ZodType> = z.infer<
    ReturnType<typeof ResponseSchema<TData>>
>;
export type UnknownResponse = Response<z.ZodUnknown>;

function makeFrameSchema<TFrameType extends FrameType, TData extends z.ZodType>(
    frameType: TFrameType,
    schema: TData,
) {
    return z.object({
        type: z.literal(frameType),
        data: schema,
    });
}

export const CloseFrameSchema = makeFrameSchema(
    FrameType.CLOSE,
    z.undefined().or(z.null()),
);
export type CloseFrame = z.infer<typeof CloseFrameSchema>;

export const CloseAckFrameSchema = makeFrameSchema(
    FrameType.CLOSE_ACK,
    z.undefined().or(z.null()),
);
export type CloseAckFrame = z.infer<typeof CloseAckFrameSchema>;

export const ProbeFrameSchema = makeFrameSchema(
    FrameType.PROBE,
    z.undefined().or(z.null()),
);
export type ProbeFrame = z.infer<typeof ProbeFrameSchema>;

export const ProbeAckFrameSchema = makeFrameSchema(
    FrameType.PROBE_ACK,
    z.undefined().or(z.null()),
);
export type ProbeAckFrame = z.infer<typeof ProbeAckFrameSchema>;

export function StreamStartFrameSchema<TData extends z.ZodType>(
    dataSchema: TData,
) {
    return makeFrameSchema(
        FrameType.STREAM_START,
        StreamStartSchema(dataSchema),
    );
}
export type StreamStartFrame<TData extends z.ZodType> = z.infer<
    ReturnType<typeof StreamStartFrameSchema<TData>>
>;
export type UnknownStreamStartFrame = StreamStartFrame<z.ZodUnknown>;

export const StreamStartResponseFrameSchema = makeFrameSchema(
    FrameType.STREAM_START_RESPONSE,
    StreamStartResponseSchema,
);
export type StreamStartResponseFrame = z.infer<
    typeof StreamStartResponseFrameSchema
>;

export function StreamDataFrameSchema<TData extends z.ZodType>(
    dataSchema: TData,
) {
    return makeFrameSchema(FrameType.STREAM_DATA, StreamDataSchema(dataSchema));
}
export type StreamDataFrame<TData extends z.ZodType> = z.infer<
    ReturnType<typeof StreamDataFrameSchema<TData>>
>;
export type UnknownStreamDataFrame = StreamDataFrame<z.ZodUnknown>;

export const StreamEndFrameSchema = makeFrameSchema(
    FrameType.STREAM_END,
    StreamEndSchema,
);
export type StreamEndFrame = z.infer<typeof StreamEndFrameSchema>;

export function MessageRequestFrameSchema<TData extends z.ZodType>(
    dataSchema: TData,
) {
    return makeFrameSchema(
        FrameType.MESSAGE_REQUEST,
        RequestSchema(dataSchema),
    );
}
export type MessageRequestFrame<TData extends z.ZodType> = z.infer<
    ReturnType<typeof MessageRequestFrameSchema<TData>>
>;
export type UnknownMessageRequestFrame = MessageRequestFrame<z.ZodUnknown>;

export function MessageResponseFrameSchema<TData extends z.ZodType>(
    dataSchema: TData,
) {
    return makeFrameSchema(
        FrameType.MESSAGE_RESPONSE,
        ResponseSchema(dataSchema),
    );
}
export type MessageResponseFrame<TData extends z.ZodType> = z.infer<
    ReturnType<typeof MessageResponseFrameSchema<TData>>
>;
export type UnknownMessageResponseFrame = MessageResponseFrame<z.ZodUnknown>;

// Frame Schema
export function FrameSchema<TData extends z.ZodType>(dataSchema: TData) {
    return z.discriminatedUnion("type", [
        CloseFrameSchema,
        CloseAckFrameSchema,
        ProbeFrameSchema,
        ProbeAckFrameSchema,
        StreamStartFrameSchema(dataSchema),
        StreamStartResponseFrameSchema,
        StreamDataFrameSchema(dataSchema),
        StreamEndFrameSchema,
        MessageRequestFrameSchema(dataSchema),
        MessageResponseFrameSchema(dataSchema),
    ]);
}
export const UnknownFrameSchema = FrameSchema(z.unknown());

export type Frame<TData extends z.ZodType = z.ZodUnknown> = z.infer<
    ReturnType<typeof FrameSchema<TData>>
>;

export type UnknownFrame = Frame<z.ZodUnknown>;

export const Frame = {
    close: (): CloseFrame => ({
        type: FrameType.CLOSE,
        data: undefined,
    }),

    closeAck: (): CloseAckFrame => ({
        type: FrameType.CLOSE_ACK,
        data: undefined,
    }),

    probe: (): ProbeFrame => ({
        type: FrameType.PROBE,
        data: undefined,
    }),

    probeAck: (): ProbeAckFrame => ({
        type: FrameType.PROBE_ACK,
        data: undefined,
    }),

    streamStart: <TData>(id: Id, path: string, data?: TData) => ({
        type: FrameType.STREAM_START,
        data: {
            id,
            path,
            data: data ?? undefined,
        },
    }),

    streamStartResponse: (
        id: Id,
        ok: boolean,
        error?: string,
    ): StreamStartResponseFrame => ({
        type: FrameType.STREAM_START_RESPONSE,
        data: {
            id,
            ok,
            error: error ? { message: error } : undefined,
        },
    }),

    streamStartResponseOk: (id: Id): StreamStartResponseFrame =>
        Frame.streamStartResponse(id, true),

    streamStartResponseError: (
        id: Id,
        error: string,
    ): StreamStartResponseFrame => Frame.streamStartResponse(id, false, error),

    streamData: <TData>(id: Id, data: TData) => ({
        type: FrameType.STREAM_DATA,
        data: {
            id,
            data,
        },
    }),

    streamEnd: (id: Id, error?: string): StreamEndFrame => ({
        type: FrameType.STREAM_END,
        data: {
            id,
            error: error ? { message: error } : undefined,
        },
    }),

    streamEndSuccess: (id: Id): StreamEndFrame => Frame.streamEnd(id),

    streamEndError: (id: Id, error: string): StreamEndFrame =>
        Frame.streamEnd(id, error),

    messageRequest: <TData>(id: Id, path: string, data: TData) => ({
        type: FrameType.MESSAGE_REQUEST,
        data: {
            id,
            path,
            data,
        },
    }),

    messageResponse: <TData>(
        id: Id,
        data?: TData,
        error?: string,
    ): UnknownFrame => ({
        type: FrameType.MESSAGE_RESPONSE,
        data: {
            id,
            data: data ?? null,
            error: error ? { message: error } : undefined,
        },
    }),

    messageResponseSuccess: <TData>(id: Id, data: TData) =>
        Frame.messageResponse(id, data),

    messageResponseError: (id: Id, error: string) =>
        Frame.messageResponse(id, undefined, error),
};

export const FrameConstants = {
    CLOSE: Frame.close(),
    CLOSE_ACK: Frame.closeAck(),
    PROBE: Frame.probe(),
    PROBE_ACK: Frame.probeAck(),
};
