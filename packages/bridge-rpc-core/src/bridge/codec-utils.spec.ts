import { describe, expect, it } from "vitest";
import { Id } from "@/id";
import { decode, decodeFrame, encode, encodeFrame } from "./codec-utils";
import { Frame } from "./frame";
import { FrameSchema } from "./frame-schema";

describe("codec-utils", () => {
    it("should encode and decode frame", () => {
        const frame = Frame.requestBodyChunk(Id.create(), encode("test"));
        const decoded = decode(encode(frame));
        const processed = FrameSchema.parse(decoded);

        expect(processed).toEqual(frame);
    });

    it("should encode and decode frames", () => {
        const frame = Frame.requestBodyChunk(Id.create(), encode("test"));
        const decoded = decodeFrame(encodeFrame(frame));
        const processed = FrameSchema.parse(decoded);

        expect(processed).toEqual(frame);
    });
});
