import z from "zod";
/**
 * Recursive type representing a serializable value.
 */
export type SerializableValue =
    | null
    | boolean
    | number
    | bigint
    | string
    | Uint8Array
    | SerializableValue[]
    | { [key: string]: SerializableValue };

export const SerializableValueSchema: z.ZodType<SerializableValue> = z.lazy(
    () => {
        return z.union([
            z.null(),
            z.boolean(),
            z.number(),
            z.bigint(),
            z.string(),
            z.instanceof(Uint8Array),
            z.array(SerializableValueSchema),
            z.record(z.string(), SerializableValueSchema),
        ]);
    },
);
