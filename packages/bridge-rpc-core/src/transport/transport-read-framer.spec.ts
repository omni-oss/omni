import { describe, expect, it } from "vitest";
import { LENGTH_PREFIX_LENGTH } from "./constants";
import { TransportReadFramer } from "./transport-read-framer";

describe("TransportReadFramer", () => {
    function combine(bytes: Uint8Array[]): Uint8Array {
        const combined = new Uint8Array(
            bytes.reduce((sum, b) => sum + b.byteLength, 0),
        );

        let offset = 0;
        for (const byte of bytes) {
            combined.set(byte, offset);
            offset += byte.byteLength;
        }

        return combined;
    }

    it("should be able to frame data in normal order", () => {
        const framer = new TransportReadFramer();
        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const data = new Uint8Array([1, 2, 3, 4]);
        new DataView(lengthPrefix.buffer).setUint32(0, 4, true);

        framer.frame(lengthPrefix);
        const framed = framer.frame(data);

        expect(framed).toEqual([data]);
    });

    it("should return false if no frame is complete", () => {
        const framer = new TransportReadFramer();
        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const data = new Uint8Array([1]);
        new DataView(lengthPrefix.buffer).setUint32(0, 4, true);

        const combined = combine([lengthPrefix, data]);

        const framed = framer.frame(combined);

        expect(framed).toBeFalsy();
    });

    it("should be able to frame data in a single received byte array", () => {
        const framer = new TransportReadFramer();
        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const data = new Uint8Array([1, 2, 3, 4]);
        new DataView(lengthPrefix.buffer).setUint32(0, 4, true);

        const combined = combine([lengthPrefix, data]);

        const framed = framer.frame(combined);

        expect(framed).toEqual([data]);
    });

    it("should be able to frame data with partial length prefix first", () => {
        const framer = new TransportReadFramer();
        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const data = new Uint8Array([1]);
        new DataView(lengthPrefix.buffer).setUint32(0, 1, true);

        const combined = combine([lengthPrefix, data]);

        const bytes = [
            combined.slice(0, 3),
            combined.slice(3, 4),
            combined.slice(4),
        ];

        let framed: Uint8Array[] = [];
        for (const byte of bytes) {
            const result = framer.frame(byte);
            if (result) {
                framed = [...framed, ...result];
            }
        }

        expect(framed).toEqual([data]);
    });

    it("should be able to frame data in a interleaved byte arrays", () => {
        const framer = new TransportReadFramer();

        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const data = new Uint8Array([1, 2, 3, 4]);
        new DataView(lengthPrefix.buffer).setUint32(0, 4, true);

        const combined = combine([lengthPrefix, data]);

        // split to 2, 4, 2 bytes
        const bytes = [
            combined.slice(0, 2),
            combined.slice(2, 6),
            combined.slice(6),
        ];

        let framed: Uint8Array[] = [];
        for (const byte of bytes) {
            const result = framer.frame(byte);
            if (result) {
                framed = [...framed, ...result];
            }
        }

        expect(framed).toEqual([data]);
    });

    it("should be able to frame multiple data in a single byte array", () => {
        const framer = new TransportReadFramer();
        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const data = new Uint8Array([1, 2, 3, 4]);
        new DataView(lengthPrefix.buffer).setUint32(0, 4, true);

        const combined = combine([lengthPrefix, data, lengthPrefix, data]);

        const framed = framer.frame(combined);

        expect(framed).toEqual([data, data]);
    });

    it("should be able to frame multiple data in an interleaved byte array", () => {
        const framer = new TransportReadFramer();
        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const data = new Uint8Array([1, 2, 3, 4]);
        new DataView(lengthPrefix.buffer).setUint32(0, 4, true);
        const combined = combine([lengthPrefix, data, lengthPrefix, data]);

        // split to 2, 4, 2, 2, 4, 2 bytes
        const bytes = [
            combined.slice(0, 2),
            combined.slice(2, 6),
            combined.slice(6, 8),
            combined.slice(8, 10),
            combined.slice(10, 14),
            combined.slice(14),
        ];

        let framed: Uint8Array[] = [];
        for (const byte of bytes) {
            const result = framer.frame(byte);
            if (result) {
                framed = [...framed, ...result];
            }
        }

        expect(framed).toEqual([data, data]);
    });

    it("should be able to frame data with zero length prefix", () => {
        const framer = new TransportReadFramer();
        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const data = new Uint8Array(0);
        new DataView(lengthPrefix.buffer).setUint32(0, 0, true);

        const combined = combine([lengthPrefix, data]);

        const framed = framer.frame(combined);

        expect(framed).toEqual([data]);
    });

    /**
     * Regression test for the prefix-incomplete early-return bug.
     *
     * When a buffer contains a *complete* frame followed by a *partial* prefix
     * for the next frame, the complete frame must still be returned.
     * Before the fix the prefix-incomplete branch returned `false`, silently
     * discarding frames that had already been collected in the same call.
     */
    it("should not drop complete frames when the next frame's prefix is partial", () => {
        const framer = new TransportReadFramer();

        const frame1Prefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        const frame1Data = new Uint8Array([1, 2, 3, 4]);
        new DataView(frame1Prefix.buffer).setUint32(0, 4, true);

        // Buffer: [complete frame 1] + [2 bytes of frame 2's prefix]
        const buf = combine([
            frame1Prefix,
            frame1Data,
            new Uint8Array([0x02, 0x00]), // partial prefix of the next frame
        ]);

        // Must return [frame1], not false
        const result = framer.frame(buf);
        expect(result).toBeTruthy();
        expect(result).toEqual([frame1Data]);

        // Supply the remaining 2 bytes of frame 2's prefix …
        const rest = framer.frame(new Uint8Array([0x00, 0x00]));
        expect(rest).toBeFalsy(); // prefix complete but body not yet

        // … and then the 2-byte body to complete frame 2.
        const frame2Data = new Uint8Array([0xaa, 0xbb]);
        const result2 = framer.frame(frame2Data);
        expect(result2).toEqual([frame2Data]);
    });
});
