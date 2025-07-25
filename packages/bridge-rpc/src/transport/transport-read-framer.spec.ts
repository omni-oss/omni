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
});
