import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
    BridgeRpc,
    type ClientHandle,
    ResponseStatusCode,
    type Service,
    type ServiceContext,
    StreamTransport,
} from "@omni-oss/bridge-rpc-core";
import type { Response } from "@omni-oss/bridge-rpc-core/client";
import type { Request } from "@omni-oss/bridge-rpc-core/server";
import { readBody } from "@omni-oss/bridge-rpc-utils/body";
import { describe, expect, test, vi } from "vitest";
import { ExecScript, type LoadedScript } from "@/exec-script";

function createTestService() {
    return {
        run: async (context: ServiceContext) => {
            const { request, response } = context;

            const requestBody = await readBody(request);

            const activeResponse = await response.start(
                ResponseStatusCode.SUCCESS,
            );
            await activeResponse.writeBodyChunk(requestBody);
            await activeResponse.end();
        },
    };
}

function makeHarness(service: Service) {
    const transport1Side = new TransformStream<Uint8Array, Uint8Array>();
    const transport2Side = new TransformStream<Uint8Array, Uint8Array>();

    const transport1 = new StreamTransport({
        input: transport1Side.readable,
        output: transport2Side.writable,
    });

    const testService = createTestService();

    const client = new BridgeRpc(transport1, testService);

    const transport2 = new StreamTransport({
        input: transport2Side.readable,
        output: transport1Side.writable,
    });
    const server = new BridgeRpc(transport2, service);

    const ret = {
        client,
        start: async () => {
            await Promise.all([client.start(), server.start()]);
        },
        stop: async () => {
            await Promise.all([client.stop(), server.stop()]);
        },
    };

    return ret;
}

function withHarness(
    service: Service,
    action: (client: ClientHandle) => Promise<void> | void,
) {
    return async () => {
        const { client, start, stop } = makeHarness(service);
        try {
            await start();
            await action(client.clientHandle);
        } finally {
            await stop();
        }
    };
}

async function drainBody(x: Response | Request) {
    const _ = await readBody(x);
}

const h = withHarness;

const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURES = join(__dirname, "__fixtures__");

const TEXT_ENCODER = new TextEncoder();

describe("ExecScript", () => {
    const postImport = vi.fn<(mod: LoadedScript) => Promise<void> | void>();
    const postImportAll =
        vi.fn<(mod: LoadedScript[]) => Promise<void> | void>();

    const service = new ExecScript({
        postImport,
        postImportAll,
    });
    const defaultScriptPath = join(FIXTURES, "default.mjs");
    const nonExistingScriptPath = join(FIXTURES, "non-existing.mjs");

    test(
        "happy path should return success",
        h(service, async (c) => {
            const request = await c.request("");
            const active = await request.start();
            await active.writeBodyChunk(
                TEXT_ENCODER.encode(JSON.stringify([defaultScriptPath])),
            );
            const response = await active.end().then((x) => x.wait());

            await drainBody(response);
            expect(response.status).toEqual(ResponseStatusCode.SUCCESS);
        }),
    );

    test(
        "error path should return not success",
        h(service, async (c) => {
            const request = await c.request("");
            const active = await request.start();
            await active.writeBodyChunk(
                TEXT_ENCODER.encode(nonExistingScriptPath),
            );
            const response = await active.end().then((x) => x.wait());

            await drainBody(response);

            expect(response.status).not.toBe(ResponseStatusCode.SUCCESS);
        }),
    );
});
