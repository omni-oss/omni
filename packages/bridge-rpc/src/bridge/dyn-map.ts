import z from "zod";

/**
 * Recursive type representing a MessagePack value (rmpv::Value equivalent).
 */
export type MsgpackValue =
    | null
    | boolean
    | number
    | bigint
    | string
    | Uint8Array
    | MsgpackValue[]
    | { [key: string]: MsgpackValue };

export const MsgpackValueSchema: z.ZodType<MsgpackValue> = z.lazy(() => {
    return z.union([
        z.null(),
        z.boolean(),
        z.number(),
        z.bigint(),
        z.string(),
        z.instanceof(Uint8Array),
        z.array(MsgpackValueSchema),
        z.record(z.string(), MsgpackValueSchema),
    ]);
});

export const DynMapSchema = z.record(z.string(), MsgpackValueSchema);

export type DynMap = z.infer<typeof DynMapSchema>;

export const HeadersSchema = DynMapSchema;
export const TrailersSchema = DynMapSchema;

export type Headers = z.infer<typeof HeadersSchema>;
export type Trailers = z.infer<typeof TrailersSchema>;
