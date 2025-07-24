import { decode, encode } from "@msgpack/msgpack";
import { describe, expect, it, vi } from "vitest";
import type { Transport } from "@/transport";
import { BridgeRpcBuilder } from "./builder";
import { FRAME_CLOSE, fReq, fResSuccess } from "./frame";

describe("BridgeRpc", () => {
    function mockTransport() {
        const mt = {
            send: vi.fn(),
            onReceive: vi.fn(),
        } satisfies Transport;

        const onReceiveHandlers = [] as ((data: Uint8Array) => void)[];

        mt.onReceive.mockImplementation((cb) => {
            onReceiveHandlers.push(cb);
        });

        return { onReceiveHandlers, ...mt };
    }

    it("should be able to close the RPC", async () => {
        const t = mockTransport();
        const rpc = new BridgeRpcBuilder(t).build();

        await rpc.close();

        t.send.mock.calls.forEach(([data]) => {
            const frame = decode(data);
            expect(frame).toEqual(FRAME_CLOSE);
        });
    });

    it("should be able to send a request", async () => {
        const t = mockTransport();

        const id = crypto.randomUUID();

        const expectedResponse = { testResponseField: "test" };

        const rpc = new BridgeRpcBuilder(t).build();

        await rpc.start();

        const actualResponseTask = rpc.requestWithId(id, "test/path", {
            testRequestField: "test",
        });

        const responseBytes = encode(fResSuccess(id, expectedResponse));
        for (const cb of t.onReceiveHandlers) {
            cb(responseBytes);
        }

        const actualResponse = await actualResponseTask;

        expect(actualResponse).toEqual(expectedResponse);
    });

    it("should be able to handle a request", async () => {
        const t = mockTransport();

        const handler = vi.fn();

        const responseData = { testResponseField: "test" };

        handler.mockImplementation(async (data) => {
            return { ...responseData, ...data };
        });

        const rpc = new BridgeRpcBuilder(t)
            .handler("test/path", handler)
            .build();

        const id = crypto.randomUUID();

        const requestData = { testRequestField: "test" };

        const requestBytes = encode(fReq(id, "test/path", requestData));

        await rpc.start();

        for (const cb of t.onReceiveHandlers) {
            cb(requestBytes);
        }
    });
});
