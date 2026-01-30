import z from "zod";
import { ProfileSchema } from "./profile";

export const SetVersionConfigSchema = z.object({
    profiles: z.array(ProfileSchema).optional().default([]),
});

export type SetVersionConfig = z.infer<typeof SetVersionConfigSchema>;
