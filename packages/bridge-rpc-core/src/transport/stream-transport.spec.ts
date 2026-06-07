import { AsyncLocalStorage } from "node:async_hooks";
import { describe, expect, it, vi } from "vitest";
import { delay } from "@/promise-utils";
import { StreamTransport } from "./stream-transport";

describe("Stream", () => {
    function createStreams(data?: Uint8Array[]) {
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
        const streams = createStreams();
        const transport = new StreamTransport(streams);

        const { lengthPrefix, data } = createData();
        await transport.send(new Uint8Array([1, 2, 3]));

        expect(streams.writtenChunks).toEqual([lengthPrefix, data]);
    });

    it("should be able to receive data", async () => {
        const { lengthPrefix, data } = createData();
        const streams = createStreams([lengthPrefix, data]);
        const fn = vi.fn();
        const transport = new StreamTransport(streams);
        transport.onReceive(fn);

        // Delay is required to ensure the data is received
        // before we start expecting it
        await delay(1); // 1ms is enough
        expect(fn).toHaveBeenCalledTimes(1);
        expect(fn).toHaveBeenCalledWith(data);
    });

    it(
        "propagates the AsyncLocalStorage store from the registration site " +
            "into a callback that fires from the stream pump set up before run()",
        async () => {
            // Reproduces the production wiring in `services/bridge-service`:
            // the transport (and therefore its `pipeTo`-based stream pump)
            // is constructed *outside* any `als.run(...)` scope, but the
            // `onReceive` callback is registered *inside* `als.run(...)`.
            // Without `bindAsyncContext`, the chunk's microtask traces back
            // to the empty async context the transport was constructed in,
            // and the callback observes `getStore() === undefined`.
            const { lengthPrefix, data } = createData();
            const streams = createStreams([lengthPrefix, data]);
            const transport = new StreamTransport(streams);

            const als = new AsyncLocalStorage<string>();
            const seenStores: Array<string | undefined> = [];

            await als.run("scoped-store", async () => {
                transport.onReceive(() => {
                    seenStores.push(als.getStore());
                });
            });

            // We are now back outside the `als.run(...)` scope.
            expect(als.getStore()).toBeUndefined();

            // Wait for the stream pump to deliver the chunk to the
            // (now-bound) callback.
            await delay(1);

            expect(seenStores).toEqual(["scoped-store"]);
        },
    );
});
