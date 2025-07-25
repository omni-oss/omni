import { afterAll, describe, expect, it } from "vitest";
import { type BridgeRpc, BridgeRpcBuilder } from "@/bridge";
import { StreamTransport } from "@/transport";

describe("Rpc to Rpc Integration", () => {
    const rpcs: ReturnType<typeof createRpcs>[] = [];

    type Rpcs = {
        rpc1: BridgeRpc;
        rpc2: BridgeRpc;
        start: () => Promise<void>;
        stop: () => Promise<void>;
    };

    afterAll(async () => {
        await Promise.all(rpcs.map((rpc) => rpc.stop()));
    });

    function createRpcs(): Rpcs {
        const transport1Side = new TransformStream<Uint8Array, Uint8Array>();
        const transport2Side = new TransformStream<Uint8Array, Uint8Array>();

        const transport1 = new StreamTransport({
            input: transport1Side.readable,
            output: transport2Side.writable,
        });

        const rpc1 = new BridgeRpcBuilder(transport1)
            .handler("rpc1test", (data: unknown) => {
                return {
                    data,
                    message: "Received data from rpc1, returning it back",
                };
            })
            .build();

        const transport2 = new StreamTransport({
            input: transport2Side.readable,
            output: transport1Side.writable,
        });

        const rpc2 = new BridgeRpcBuilder(transport2)
            .handler("rpc2test", (data: unknown) => {
                return {
                    data,
                    message: "Received data from rpc2, returning it back",
                };
            })
            .build();

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

        rpcs.push(ret);

        return ret;
    }

    it("should be able to probe the RPC", async () => {
        const { rpc2: rpc, start } = createRpcs();

        await start();

        const probe = rpc.probe(100);

        expect(await probe).toBe(true);
    });

    it("should be able to send and receive data", async () => {
        const { rpc1, start } = createRpcs();

        await start();

        const reqData = { test: "test" };

        const data = await rpc1.request("rpc2test", reqData);

        expect(data).toEqual({
            data: reqData,
            message: "Received data from rpc2, returning it back",
        });
    });
});
