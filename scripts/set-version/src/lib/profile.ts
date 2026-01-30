import z from "zod";
import { FormatSchema } from "./format";

const ProfileBaseSchema = z.object({
    files: z.string().array(),
});

export type ProfileBase = z.infer<typeof ProfileBaseSchema>;

export const PathProfileSchema = ProfileBaseSchema.extend({
    path: z.array(z.union([z.string(), z.number()])),
    format: FormatSchema.optional(),
});

export const TaggedPathProfileSchema = PathProfileSchema.extend({
    type: z.literal("path"),
});

export type PathProfile = z.infer<typeof PathProfileSchema>;
export type TaggedPathProfile = z.infer<typeof TaggedPathProfileSchema>;

export const RegexProfileSchema = ProfileBaseSchema.extend({
    pattern: z.string(),
    flags: z.string().optional(),
    capture_group: z.string().optional(),
});

export const TaggedRegexProfileSchema = RegexProfileSchema.extend({
    type: z.literal("regex"),
});

export type RegexProfile = z.infer<typeof RegexProfileSchema>;
export type TaggedRegexProfile = z.infer<typeof TaggedRegexProfileSchema>;

export const ProfileSchema = z.discriminatedUnion("type", [
    TaggedPathProfileSchema,
    TaggedRegexProfileSchema,
]);

export type Profile = z.infer<typeof ProfileSchema>;
