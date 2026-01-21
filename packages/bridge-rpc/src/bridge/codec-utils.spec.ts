import { describe, expect, it } from "vitest";
import { Id } from "@/id";
import { decode, encode } from "./codec-utils";
import { Frame } from "./frame";
import { FrameSchema } from "./frame-schema";

describe("codec-utils", () => {
    it("should encode and decode frame", () => {
        const frame = Frame.requestBodyChunk(Id.create(), encode("test"));
        const decoded = decode(encode(frame));
        const processed = FrameSchema.parse(decoded);

        expect(processed).toEqual(frame);
    });
});
