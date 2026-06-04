import { join } from "node:path/win32";
import {
    type BridgeRpc,
    ResponseStatusCode,
    StreamTransport,
} from "@omni-oss/bridge-rpc-core";
import { describe, expect, it } from "vitest";
import { createRpcInstance } from "..";

const __dirname = import.meta.dirname;

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder();

describe("integration test", {
    timeout: 10_000,
}, () => {
    it(
        "should respond to /exec-script requests",
        withRpcs(async (rpc) => {
            const request = await rpc.clientHandle
                .request("/exec-script")
                .then((req) => req.start());
            const scriptPath = join(__dirname, "__fixtures__", "test.mjs");
            await request.writeBodyChunk(json(scriptPath));
            const end = await request.end().then((x) => x.wait());

            const body = await readAll(end.readBody());

            if (end.status !== ResponseStatusCode.SUCCESS) {
                console.error(
                    "Error response body:",
                    TEXT_DECODER.decode(body),
                );
            }

            expect(end.status).toEqual(ResponseStatusCode.SUCCESS);
        }),
    );
});

function createRpcs(): Rpcs {
    const rctSide = new TransformStream<Uint8Array, Uint8Array>();
    const rstSide = new TransformStream<Uint8Array, Uint8Array>();

    const rct = new StreamTransport({
        input: rctSide.readable,
        output: rstSide.writable,
    });

    const rst = new StreamTransport({
        input: rstSide.readable,
        output: rctSide.writable,
    });

    const rpc1 = createRpcInstance(rct);
    const rpc2 = createRpcInstance(rst);

    return {
        rpc1,
        rpc2,
        start: () =>
            Promise.all([rpc1.start(), rpc2.start()]).then(() => void 0),
        stop: () => Promise.all([rpc1.stop(), rpc2.stop()]).then(() => void 0),
    };
}

function withRpcs(action: (rpc1: BridgeRpc, rpc2: BridgeRpc) => Promise<void>) {
    return async () => {
        const { rpc1, rpc2, start, stop } = createRpcs();
        try {
            await start();
            await delay(10); // wait for the rpcs to be ready
            await action(rpc1, rpc2);
            await delay(10);
        } finally {
            await stop();
        }
    };
}

type Rpcs = {
    rpc1: BridgeRpc;
    rpc2: BridgeRpc;
    start: () => Promise<void>;
    stop: () => Promise<void>;
};

function json(unknown: unknown) {
    return TEXT_ENCODER.encode(JSON.stringify(unknown));
}

function delay(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

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
