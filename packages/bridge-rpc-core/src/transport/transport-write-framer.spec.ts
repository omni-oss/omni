import { describe, expect, it } from "vitest";
import { TransportWriteFramer } from "./transport-write-framer";

describe("TransportWriteFramer", () => {
    it("should be able to frame data", () => {
        const framer = new TransportWriteFramer();
        const data = new Uint8Array([1, 2, 3, 4]);
        const [lengthPrefix, framedData] = framer.frame(data);

        expect(lengthPrefix).toEqual(new Uint8Array([4, 0, 0, 0]));
        expect(framedData).toEqual(data);
    });

    it("should be able to frame data with zero length", () => {
        const framer = new TransportWriteFramer();
        const data = new Uint8Array(0);
        const [lengthPrefix, framedData] = framer.frame(data);

        expect(lengthPrefix).toEqual(new Uint8Array([0, 0, 0, 0]));
        expect(framedData).toEqual(data);
    });
});
