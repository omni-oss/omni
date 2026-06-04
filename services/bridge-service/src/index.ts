import {
    createRpcInstance as baseCreateRpcInstance,
    createStdioRpcInstance as baseCreateStdioRpcInstnace,
} from "@omni-oss/bridge-rpc-bootstrap";
import type { Transport } from "@omni-oss/bridge-rpc-core";
import { createServices } from "./services";

export function createRpcInstance(transport: Transport) {
    return baseCreateRpcInstance(transport, {
        services: createServices(),
    });
}

export function createStudioRpcInstance() {
    return baseCreateStdioRpcInstnace({
        services: createServices(),
    });
}
