import { describe, expect, it } from "vitest";
import { BridgeRpcBuilder } from "@/bridge";
import { StreamTransport } from "@/transport";

describe("Rpc to Rpc Integration", () => {
    function createRpcs() {
        // One duplex pipe
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

        return {
            transport1,
            rpc1,
            transport2,
            rpc2,
            start: () => {
                return Promise.all([rpc1.start(), rpc2.start()]);
            },
            stop: () => {
                return Promise.all([rpc1.stop(), rpc2.stop()]);
            },
        };
    }

    it("should be able to send and receive data", async () => {
        const { rpc1, start, stop } = createRpcs();

        await start();

        const reqData = { test: "test" };

        const data = await rpc1.request("rpc2test", reqData);

        await stop();

        expect(data).toEqual({
            data: reqData,
            message: "Received data from rpc2, returning it back",
        });
    });
});
