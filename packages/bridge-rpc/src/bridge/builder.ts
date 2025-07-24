import type { Transport } from "../transport";
import {
    type BridgeRequestHandler,
    BridgeRpc,
    type UnknownBridgeRequestHandler,
} from "./bridge-impl";

export class BridgeRpcBuilder {
    private handlers = new Map<string, UnknownBridgeRequestHandler>();

    static create(transport: Transport) {
        return new BridgeRpcBuilder(transport);
    }

    constructor(private readonly transport: Transport) {}

    handler<TRequest, TResponse>(
        path: string,
        handler: BridgeRequestHandler<TRequest, TResponse>,
    ) {
        this.handlers.set(path, handler as UnknownBridgeRequestHandler);

        return this;
    }

    build(): BridgeRpc {
        return new BridgeRpc({
            transport: this.transport,
            handlers: this.handlers,
        });
    }
}
