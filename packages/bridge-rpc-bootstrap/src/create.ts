import process from "node:process";
import { Readable, Writable } from "node:stream";
import {
    BridgeRpc,
    type Service as RpcService,
    StreamTransport,
    type Transport,
} from "@omni-oss/bridge-rpc-core";

import { type HandlerFn, Router } from "@omni-oss/bridge-rpc-router";

export type { Service as RpcService } from "@omni-oss/bridge-rpc-core";
export type { HandlerFn } from "@omni-oss/bridge-rpc-router";

export type Handler = {
    path: string;
    handler: HandlerFn;
};

export type Service = {
    path: string;
    service: RpcService;
};

export type RpcConfig = {
    services?: (Handler | Service)[];
};

export function createStdioRpcInstance(config?: RpcConfig) {
    const transport = new StreamTransport({
        input: Readable.toWeb(process.stdin, {
            type: "bytes",
        }) as ReadableStream<Uint8Array>,
        output: Writable.toWeb(process.stdout),
    });

    return createRpcInstance(transport, config);
}

export function createRpcInstance(transport: Transport, config?: RpcConfig) {
    config ??= {};
    config.services ??= [];

    const router = new Router();

    for (const item of config.services) {
        if (router.hasService(item.path)) {
            throw new Error(`Duplicate service path: ${item.path}`);
        }

        if ("handler" in item) {
            router.addHandler(item.path, item.handler);
        } else if ("service" in item) {
            router.addService(item.path, item.service);
        }
    }

    const rpc = new BridgeRpc(transport, router);

    return rpc;
}
