import z from "zod";

export const Uint16Schema = z.int().gte(0).lt(65_536);
