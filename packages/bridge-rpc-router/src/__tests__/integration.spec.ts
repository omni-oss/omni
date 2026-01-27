import {
    BridgeRpc,
    ResponseStatusCode,
    type Service,
    StreamTransport,
} from "@omni-oss/bridge-rpc";
import { describe, expect, it } from "vitest";
import { Router } from "@/index";

const TEST_DATA = bytesFromObject({ test: "data" });

describe("BridgeRpc to Router Integration", () => {
    it(
        "should be able to run a service",
        withRpcs(async (rpc1) => {
            const request = await (await rpc1.request("/test")).start();
            await request.writeBodyChunk(TEST_DATA);
            const response = await (await request.end()).wait();
            const body = await readAll(response.readBody());

            expect(response.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(objectFromBytes(body)).toEqual(objectFromBytes(TEST_DATA));
        }),
    );

    it(
        "should be able to handle no service found",
        withRpcs(async (rpc1) => {
            const request = await (await rpc1.request("test")).start();
            await request.writeBodyChunk(TEST_DATA);
            const response = await (await request.end()).wait();

            await readAll(response.readBody());

            expect(response.status).toEqual(
                ResponseStatusCode.NO_HANDLER_FOR_PATH,
            );
        }),
    );
});

type Rpcs = {
    rpc1: BridgeRpc;
    rpc2: BridgeRpc;
    start: () => Promise<void>;
    stop: () => Promise<void>;
};

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

function createRpcs(
    service1: Service = createTestRouter(),
    service2: Service = createTestRouter(),
): Rpcs {
    const transport1Side = new TransformStream<Uint8Array, Uint8Array>();
    const transport2Side = new TransformStream<Uint8Array, Uint8Array>();

    const transport1 = new StreamTransport({
        input: transport1Side.readable,
        output: transport2Side.writable,
    });

    const rpc1 = new BridgeRpc(transport1, service1);

    const transport2 = new StreamTransport({
        input: transport2Side.readable,
        output: transport1Side.writable,
    });

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

function withRpcs(action: (rpc1: BridgeRpc, rpc2: BridgeRpc) => Promise<void>) {
    return async () => {
        const { rpc1, rpc2, start, stop } = createRpcs();
        try {
            await start();
            await action(rpc1, rpc2);
        } finally {
            await stop();
        }
    };
}

function createTestRouter(path = "/test") {
    const router = new Router();

    return router.addHandler(path, async (context) => {
        const { request, response } = context;
        const body = await readAll(request.readBody());
        const activeResponse = await response.start(ResponseStatusCode.SUCCESS);
        if (body.length > 0) {
            await activeResponse.writeBodyChunk(body);
        }
        await activeResponse.end();
    });
}

function textFromBytes(bytes: Uint8Array): string {
    return new TextDecoder().decode(bytes);
}

function objectFromBytes(bytes: Uint8Array): unknown {
    return JSON.parse(textFromBytes(bytes));
}

function bytesFromText(text: string): Uint8Array {
    return new TextEncoder().encode(text);
}

function bytesFromObject(obj: unknown): Uint8Array {
    return bytesFromText(JSON.stringify(obj));
}
