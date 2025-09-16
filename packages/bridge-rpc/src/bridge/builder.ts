import type { Transport } from "../transport";
import {
    type BridgeRequestHandler,
    BridgeRpc,
    type UnknownBridgeRequestHandler,
} from "./bridge-impl";
import type { Push } from "./type-utils";

export class BridgeRpcBuilder<THandlers extends [...string[]] = []> {
    private handlers = new Map<string, UnknownBridgeRequestHandler>();

    static create(transport: Transport) {
        return new BridgeRpcBuilder(transport);
    }

    constructor(private readonly transport: Transport) {}

    handler<const TPath extends string, TRequest, TResponse>(
        path: TPath,
        handler: BridgeRequestHandler<TRequest, TResponse>,
    ) {
        this.handlers.set(path, handler as UnknownBridgeRequestHandler);

        return this as unknown as BridgeRpcBuilder<Push<THandlers, TPath>>;
    }

    build(): BridgeRpc<THandlers> {
        return new BridgeRpc({
            transport: this.transport,
            handlers: this.handlers,
        });
    }
}
