import type { Transport } from "../transport";
import {
    type BridgeRequestHandler,
    BridgeRpc,
    type BridgeStreamHandler,
    type UnknownBridgeRequestHandler,
    type UnknownBridgeStreamHandler,
} from "./bridge-impl";
import type { Push } from "./type-utils";

export class DuplicatePathError extends Error {
    constructor(public readonly path: string) {
        super(
            `Duplicate path is registered as a stream and request handler: "${path}", path must be unique`,
        );
    }
}

export class BridgeRpcBuilder<
    TRequestHandlers extends [...string[]] = [],
    TStreamHandlers extends [...string[]] = [],
> {
    private requestHandlers = new Map<string, UnknownBridgeRequestHandler>();
    private streamHandlers = new Map<string, UnknownBridgeStreamHandler>();

    static create(transport: Transport) {
        return new BridgeRpcBuilder(transport);
    }

    constructor(private readonly transport: Transport) {}

    requestHandler<const TPath extends string, TRequest, TResponse>(
        path: TPath,
        handler: BridgeRequestHandler<TRequest, TResponse>,
    ) {
        this.requestHandlers.set(path, handler as UnknownBridgeRequestHandler);

        return this as unknown as BridgeRpcBuilder<
            Push<TRequestHandlers, TPath>,
            TStreamHandlers
        >;
    }

    streamHandler<const TPath extends string, TStartData, TStreamData>(
        path: TPath,
        handler: BridgeStreamHandler<TStartData, TStreamData>,
    ) {
        this.streamHandlers.set(path, handler as UnknownBridgeStreamHandler);

        return this as unknown as BridgeRpcBuilder<
            TRequestHandlers,
            Push<TStreamHandlers, TPath>
        >;
    }

    build(): BridgeRpc<TRequestHandlers, TStreamHandlers> {
        const duplicate = getDuplicatePath(
            this.requestHandlers,
            this.streamHandlers,
        );
        if (duplicate !== null) {
            throw new DuplicatePathError(duplicate);
        }

        return new BridgeRpc({
            transport: this.transport,
            requestHandlers: this.requestHandlers,
            streamHandlers: this.streamHandlers,
        });
    }
}

function getDuplicatePath(
    map1: Map<string, unknown>,
    map2: Map<string, unknown>,
): string | null {
    const keys = new Set([...map1.keys(), ...map2.keys()]);

    for (const key of keys) {
        if (map1.has(key) && map2.has(key)) {
            return key;
        }
    }
    return null;
}
