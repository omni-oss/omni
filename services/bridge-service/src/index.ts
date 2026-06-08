import {
    createRpcInstance as baseCreateRpcInstance,
    createStdioRpcInstance as baseCreateStdioRpcInstnace,
    type RpcConfig,
} from "@omni-oss/bridge-rpc-bootstrap";
import type { Transport } from "@omni-oss/bridge-rpc-core";
import { createServices } from "./services";

export function createRpcInstance(transport: Transport, config?: RpcConfig) {
    return baseCreateRpcInstance(transport, {
        services: [...createServices(), ...(config?.services || [])],
    });
}

export function createStudioRpcInstance(config?: RpcConfig) {
    return baseCreateStdioRpcInstnace({
        services: [...createServices(), ...(config?.services || [])],
    });
}
