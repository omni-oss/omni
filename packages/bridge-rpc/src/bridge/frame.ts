import * as z from "zod";

export function BridgeRequestSchema<TItemType extends z.ZodType>(
    schema: TItemType,
) {
    return z.object({
        id: z.string(),
        path: z.string(),
        data: schema,
    });
}

export const UnknownBridgeRequestSchema = BridgeRequestSchema(z.unknown());

export type UnknownBridgeRequest = z.infer<typeof UnknownBridgeRequestSchema>;

export const BridgeErrorDataSchema = z.object({
    error_message: z.string(),
});

export type BridgeErrorData = z.infer<typeof BridgeErrorDataSchema>;

export function BridgeResponseSchema<TItemType extends z.ZodType>(
    schema: TItemType,
) {
    return z.object({
        id: z.string(),
        data: z.nullish(schema),
        error: z.nullish(BridgeErrorDataSchema),
    });
}

export const UnknownBridgeResponseSchema = BridgeResponseSchema(z.unknown());

export type UnknownBridgeResponse = z.infer<typeof UnknownBridgeResponseSchema>;

export function BridgeFrameSchema<TItemType extends z.ZodType>(
    schema: TItemType,
) {
    return z.discriminatedUnion("type", [
        z.object({
            type: z.literal("internal_op"),
            content: z.enum(["close", "close_ack"]),
        }),
        z.object({
            type: z.literal("response"),
            content: BridgeResponseSchema(schema),
        }),
        z.object({
            type: z.literal("request"),
            content: BridgeRequestSchema(schema),
        }),
    ]);
}

export const UnknownBridgeFrameSchema = BridgeFrameSchema(z.unknown());

export type UnknownBridgeFrame = z.infer<typeof UnknownBridgeFrameSchema>;

export const FRAME_CLOSE = {
    type: "internal_op",
    content: "close",
} satisfies UnknownBridgeFrame;

export function fClose() {
    return FRAME_CLOSE;
}

export function fResError(id: string, errorMessage: string) {
    return {
        type: "response",
        content: {
            id,
            data: null,
            error: {
                error_message: errorMessage,
            },
        },
    } satisfies UnknownBridgeFrame;
}

export function fResSuccess<TResponse>(id: string, data: TResponse) {
    return {
        type: "response",
        content: {
            id,
            data,
            error: null,
        },
    } satisfies UnknownBridgeFrame;
}

export function fReq<TRequest>(id: string, path: string, data: TRequest) {
    return {
        type: "request",
        content: {
            id,
            path,
            data,
        },
    } satisfies UnknownBridgeFrame;
}
