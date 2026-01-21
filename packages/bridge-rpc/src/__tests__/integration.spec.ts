import { describe, expect, it } from "vitest";
import { BridgeRpc } from "@/bridge";
import { decode, encode } from "@/bridge/codec-utils";
import type { ServiceContext } from "@/bridge/service";
import { ResponseStatusCode } from "@/bridge/status-code";
import { delay } from "@/promise-utils";
import { StreamTransport } from "@/transport";

describe("Rpc to Rpc Integration", () => {
    type Rpcs = {
        rpc1: BridgeRpc;
        rpc2: BridgeRpc;
        start: () => Promise<void>;
        stop: () => Promise<void>;
    };

    function createTestService() {
        return {
            run: async (context: ServiceContext) => {
                const { request, response } = context;

                const requestBody = await readAll(request.readBody());

                const activeResponse = await response.start(
                    ResponseStatusCode.SUCCESS,
                );
                await activeResponse.writeBodyChunk(requestBody);
                await activeResponse.end();
            },
        };
    }

    async function readAll(
        gen: AsyncIterable<Uint8Array>,
    ): Promise<Uint8Array> {
        const chunks: Uint8Array[] = [];
        for await (const chunk of gen) {
            chunks.push(chunk);
        }
        const wholeChunk = new Uint8Array(
            chunks.reduce((acc, c) => acc + c.length, 0),
        );

        for (const chunk of chunks) {
            wholeChunk.set(chunk, wholeChunk.length - chunk.length);
        }

        return wholeChunk;
    }

    function createRpcs(): Rpcs {
        const transport1Side = new TransformStream<Uint8Array, Uint8Array>();
        const transport2Side = new TransformStream<Uint8Array, Uint8Array>();

        const transport1 = new StreamTransport({
            input: transport1Side.readable,
            output: transport2Side.writable,
        });

        const service1 = createTestService();

        const rpc1 = new BridgeRpc(transport1, service1);

        const transport2 = new StreamTransport({
            input: transport2Side.readable,
            output: transport1Side.writable,
        });
        const service2 = createTestService();
        const rpc2 = new BridgeRpc(transport2, service2);

        const ret = {
            rpc1,
            rpc2,
            start: async () => {
                await Promise.all([rpc1.start(), rpc2.start()]);
            },
            stop: async () => {
                await Promise.all([rpc1.stop(), rpc2.stop()]);
            },
        };

        return ret;
    }

    async function run(
        action: (rpc1: BridgeRpc, rpc2: BridgeRpc) => Promise<void>,
    ) {
        const { rpc1, rpc2, start, stop } = createRpcs();
        try {
            await start();
            await action(rpc1, rpc2);
        } finally {
            await stop();
        }
    }

    it("should be able to handle ping/pong cycle", () =>
        run(async (rpc) => {
            await delay(10);
            const pingResult = await rpc.ping(100);
            await delay(10);

            expect(pingResult).toBe(true);
        }));

    it("should be able to send and receive data", () =>
        run(async (rpc) => {
            const reqData = { test: "test" };
            const reqDataBytes = encode(reqData);

            const activeRequest = await (await rpc.request("rpc2test")).start();

            await activeRequest.writeBodyChunk(reqDataBytes);
            const response = await activeRequest.end();
            const activeResponse = await response.wait();
            const responseBody = await readAll(activeResponse.readBody());

            expect(activeResponse.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(decode(responseBody)).toEqual(reqData);
        }));
});
