import { describe, expect, it, vi } from "vitest";
import { delay } from "@/promise-utils";
import { StdioTransport } from "./stdio-transport";

describe("StdioTransport", () => {
    function createStdio(data?: Uint8Array[]) {
        const input = new ReadableStream<Uint8Array>({
            pull(controller) {
                for (const chunk of data ?? []) {
                    controller.enqueue(chunk);
                }

                controller.close();
            },
        });

        const writtenChunks: Uint8Array[] = [];
        const output = new WritableStream<Uint8Array>({
            write(chunk) {
                writtenChunks.push(chunk);
            },
        });

        return {
            input,
            output,
            writtenChunks,
        };
    }

    function createData() {
        const lengthPrefix = new Uint8Array(4);
        const data = new Uint8Array([1, 2, 3]);

        new DataView(lengthPrefix.buffer).setUint32(0, data.byteLength, true);

        return {
            lengthPrefix,
            data,
        };
    }

    it("should be able to send data", async () => {
        const stdio = createStdio();
        const transport = new StdioTransport(stdio);

        const { lengthPrefix, data } = createData();
        await transport.send(new Uint8Array([1, 2, 3]));

        expect(stdio.writtenChunks).toEqual([lengthPrefix, data]);
    });

    it("should be able to receive data", async () => {
        const { lengthPrefix, data } = createData();
        const stdio = createStdio([lengthPrefix, data]);
        const fn = vi.fn();
        const transport = new StdioTransport(stdio);
        transport.onReceive(fn);

        // Delay is required to ensure the data is received
        // before we start expecting it
        await delay(1); // 1ms is enough
        expect(fn).toBeCalledTimes(1);
        expect(fn).toBeCalledWith(data);
    });
});
