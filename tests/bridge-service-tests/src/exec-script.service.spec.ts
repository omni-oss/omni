import { type ChildProcess, spawn } from "node:child_process";
import { join } from "node:path";
import { Readable, Writable } from "node:stream";
import { createRpcInstance } from "@omni-oss/bridge-rpc-bootstrap";
import {
    type BridgeRpc,
    ResponseStatusCode,
    StreamTransport,
} from "@omni-oss/bridge-rpc-core";
import { RUNTIME } from "@omni-oss/runtime-utils";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const __dirname = import.meta.dirname;

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder();

let rpcProcess: ChildProcess;
let rpc: BridgeRpc;

beforeAll(async () => {
    rpcProcess = spawn(
        RUNTIME,
        [
            join(
                __dirname,
                "../../../services/bridge-service/dist/bridge-service-cli.mjs",
            ),
            "run",
        ],
        {
            stdio: "pipe",
        },
    );

    if (!rpcProcess.stdout || !rpcProcess.stdin) {
        throw new Error("Failed to spawn RPC process with piped stdio");
    }
    await delay(10);

    if (rpcProcess.exitCode !== null) {
        throw new Error(
            `RPC process exited prematurely with code ${rpcProcess.exitCode}`,
        );
    }

    rpcProcess.on("error", (err) => {
        console.error("RPC process error:", err);
    });

    rpcProcess.on("exit", (code, signal) => {
        console.log(
            `RPC process exited with code ${code} and signal ${signal}`,
        );
    });

    rpcProcess.stderr?.on("data", (data) => {
        console.error(`RPC process stderr: ${data}`);
    });

    rpcProcess.stderr?.on("error", (err) => {
        console.error("Error reading RPC process stderr:", err);
    });

    rpcProcess.stdout?.on("error", (err) => {
        console.error("Error reading RPC process stdout:", err);
    });

    rpcProcess.stdin?.on("error", (err) => {
        console.error("Error writing to RPC process stdin:", err);
    });

    const transport = new StreamTransport({
        input: Readable.toWeb(rpcProcess.stdout) as ReadableStream<Uint8Array>,
        output: Writable.toWeb(rpcProcess.stdin),
    });

    rpc = createRpcInstance(transport);

    await rpc.start();
    await delay(1);
});

afterAll(() => rpc.stop());

describe("/exec-script", {
    timeout: 10_000,
}, () => {
    it("should respond to requests", async () => {
        const request = await rpc.clientHandle
            .request("/exec-script")
            .then((req) => req.start());
        const scriptPath = join(__dirname, "__fixtures__", "test.mjs");
        await request.writeBodyChunk(json(scriptPath));
        const end = await request.end().then((x) => x.wait());

        const body = await readAll(end.readBody());
        if (!end.status.equals(ResponseStatusCode.SUCCESS)) {
            console.error("Error response body:", TEXT_DECODER.decode(body));
        }

        expect(end.status).toEqual(ResponseStatusCode.SUCCESS);
    });
});

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
