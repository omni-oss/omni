import { describe, expect, it, vi } from "vitest";
import { withDelay } from "@/promise-utils";
import type { Transport } from "@/transport";
import { BridgeRpcBuilder } from "./builder";
import { Frame, FrameConstants } from "./frame";
import { Id } from "./id";
import { decode, encode } from "./utils";

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

    it("should be able to stop the RPC", async () => {
        const t = mockTransport();
        const rpc = new BridgeRpcBuilder(t).build();

        await rpc.stop();

        t.send.mock.calls.forEach(([data]) => {
            const frame = decode(data);
            expect(frame).toEqual(FrameConstants.CLOSE);
        });
    });

    it("should be able to probe the RPC", async () => {
        const t = mockTransport();
        const rpc = new BridgeRpcBuilder(t).build();

        await rpc.start();

        const probe = withDelay(rpc.probe(100), 1);

        const probeAckBytes = encode(FrameConstants.PROBE_ACK);
        for (const cb of t.onReceiveHandlers) {
            cb(probeAckBytes);
        }

        expect(await probe).toBe(true);
        expect(rpc.hasPendingProbe()).toBe(false);

        await rpc.stop();
    });

    it("should be able to send a request", async () => {
        const t = mockTransport();
        const id = Id.create();
        const expectedResponse = { testResponseField: "test" };

        const rpc = new BridgeRpcBuilder(t).build();

        await rpc.start();

        const requestData = { testRequestField: "test" };
        const actualResponseTask = withDelay(
            rpc.requestWithId(id, "test/path", requestData),
            1,
        );

        const responseBytes = encode(
            Frame.messageResponseSuccess(id, expectedResponse),
        );

        for (const cb of t.onReceiveHandlers) {
            cb(responseBytes);
        }

        expect(await actualResponseTask).toEqual(expectedResponse);

        await rpc.stop();
    });

    it("should be able to handle a request", async () => {
        const t = mockTransport();
        const handler = vi.fn();
        const responseData = { testResponseField: "test" };

        handler.mockImplementation(async (context) => {
            return { ...responseData, ...context?.data };
        });

        const rpc = new BridgeRpcBuilder(t)
            .requestHandler("test/path", handler)
            .build();

        const requestData = { testRequestField: "test" };
        const requestBytes = encode(
            Frame.messageRequest(Id.create(), "test/path", requestData),
        );

        await rpc.start();

        // send request to the RPC
        for (const cb of t.onReceiveHandlers) {
            cb(requestBytes);
        }

        expect(handler).toBeCalledWith({ data: requestData });

        await rpc.stop();
    });
});
