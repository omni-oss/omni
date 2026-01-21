import { describe, expect, it, vi } from "vitest";
import { Id } from "@/id";
import type { Transport } from "@/transport";
import { BridgeRpc } from "./bridge-impl";
import { decode, encode } from "./codec-utils";
import { Frame } from "./frame";
import { FrameType } from "./frame-schema";
import type { ServiceContext } from "./service";
import { ResponseStatusCode } from "./status-code";

// Mock Constants
const TEST_DATA = "test_data";
const TEST_PATH = "test_path";
const testDataBytes = encode(TEST_DATA);

describe("BridgeRpc", () => {
    // Helper to create a Mock Transport
    function createMockTransport() {
        const mockTransport = {
            send: vi.fn(),
            onReceive: vi.fn(),
        } satisfies Transport;

        const onReceiveHandlers = [] as ((data: Uint8Array) => void)[];

        mockTransport.onReceive.mockImplementation((cb) => {
            onReceiveHandlers.push(cb);
        });

        return { onReceiveHandlers, ...mockTransport };
    }

    // Helper to create a Mock Service
    const createMockService = () => ({
        run: vi.fn().mockImplementation(async (ctx: ServiceContext) => {
            const { request, response } = ctx;
            const requestBytes = await readAll(request.readBody());

            const activeResponse = await response.start(
                ResponseStatusCode.SUCCESS,
            );
            await activeResponse.writeBodyChunk(requestBytes);
            await activeResponse.end();

            return undefined;
        }),
    });

    const sleep = (ms: number) =>
        new Promise((resolve) => setTimeout(resolve, ms));

    it("should send close frame when stopped", async () => {
        const transport = createMockTransport();
        const service = createMockService();
        const rpc = new BridgeRpc(transport, service);

        await expect(rpc.start()).resolves.toBeUndefined();
        await expect(rpc.stop()).resolves.toBeUndefined();

        expect(transport.send).toHaveBeenCalled();
        // biome-ignore lint/style/noNonNullAssertion: should have value
        const lastCallBytes = transport.send.mock.calls[0]![0];
        const decodedFrame = decode(lastCallBytes) as Frame;

        expect(decodedFrame).toEqual(Frame.close());
    });

    it("should handle ping/pong cycle", async () => {
        const transport = createMockTransport();
        const service = createMockService();
        const rpc = new BridgeRpc(transport, service);

        // When RPC sends a PING, we simulate the other side sending a PONG back
        transport.send.mockImplementation(async (bytes: Uint8Array) => {
            const frame = decode(bytes) as Frame;
            if (frame.type === FrameType.PING) {
                // Simulate network delay then send PONG
                setTimeout(() => {
                    const pongFrame = Frame.pong();
                    // biome-ignore lint/style/noNonNullAssertion: should have value
                    const receiveHandler = transport.onReceiveHandlers[0]!;
                    receiveHandler(encode(pongFrame));
                }, 10);
            }
        });

        await expect(rpc.start()).resolves.toBeUndefined();
        // This should resolve when the PONG is received via transport.onReceive
        await expect(rpc.ping(1000)).resolves.toBeTruthy();

        await expect(rpc.stop()).resolves.toBeUndefined();
    });

    it("client sending request and receiving response", async () => {
        const transport = createMockTransport();
        const service = createMockService();
        const rpc = new BridgeRpc(transport, service);
        const reqId = Id.create();

        await rpc.start();

        // 1. Setup transport to simulate a server response when it receives request frames
        transport.send.mockImplementation(async (bytes: Uint8Array) => {
            const frame = decode(bytes) as Frame;

            // If we see the end of the request, simulate the server's response
            if (frame.type === FrameType.REQUEST_END) {
                // biome-ignore lint/style/noNonNullAssertion: should have value
                const receiveHandler = transport.onReceiveHandlers[0]!;
                const frames = [
                    Frame.responseStart(reqId, ResponseStatusCode.SUCCESS),
                    Frame.responseBodyChunk(reqId, testDataBytes),
                    Frame.responseEnd(reqId),
                ];

                for (const frame of frames) {
                    receiveHandler(encode(frame));
                }
            }
        });

        const pendingRequest = await rpc.requestWithId(reqId, TEST_PATH);
        const requestHandle = await pendingRequest.start();
        await requestHandle.writeBodyChunk(testDataBytes);
        const response = await (await requestHandle.end()).wait();

        expect(response.status).toEqual(ResponseStatusCode.SUCCESS);
        const body = await readAll(response.readBody());

        expect(body).toEqual(testDataBytes);
        await expect(rpc.stop()).resolves.toBeUndefined();
    });

    it("server handling request and sending response", async () => {
        const transport = createMockTransport();
        const reqId = Id.create();

        // Logic inside the mock service to handle the incoming request
        const service = {
            run: vi.fn().mockImplementation(async (ctx: ServiceContext) => {
                const { request, response } = ctx;
                // Read the request body
                const body = await readAll(request.readBody());

                // Send back response
                const respWriter = await response.start(
                    ResponseStatusCode.SUCCESS,
                );

                await respWriter.writeBodyChunk(body);
                await respWriter.end();

                expect(request.path).toBe(TEST_PATH);
            }),
        };

        const rpc = new BridgeRpc(transport, service);
        await rpc.start();

        // Simulate incoming request frames from the "network"
        // biome-ignore lint/style/noNonNullAssertion: should have value
        const receiveHandler = transport.onReceiveHandlers[0]!;

        const frames = [
            Frame.requestStart(reqId, TEST_PATH),
            Frame.requestBodyChunk(reqId, testDataBytes),
            Frame.requestEnd(reqId),
        ];

        for (const frame of frames) {
            receiveHandler(encode(frame));
        }

        // Allow microtasks to process (service.run is called in background)
        await sleep(50);

        // Verify transport sent response frames back
        const sentFrameTypes = transport.send.mock.calls.map(
            (call) => (decode(call[0]) as Frame).type,
        );

        expect(sentFrameTypes).toContain(FrameType.RESPONSE_START);
        expect(sentFrameTypes).toContain(FrameType.RESPONSE_BODY_CHUNK);
        expect(sentFrameTypes).toContain(FrameType.RESPONSE_END);

        await expect(rpc.stop()).resolves.toBeUndefined();
    });
});

async function readAll(gen: AsyncIterable<Uint8Array>): Promise<Uint8Array> {
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
