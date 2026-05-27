import z from "zod";
import { SerializableValueSchema } from "./serializable-value";

export const DynMapSchema = z.record(z.string(), SerializableValueSchema);

export type DynMap = z.infer<typeof DynMapSchema>;

export const HeadersSchema = DynMapSchema;
export const TrailersSchema = DynMapSchema;

export type Headers = z.infer<typeof HeadersSchema>;
export type Trailers = z.infer<typeof TrailersSchema>;
