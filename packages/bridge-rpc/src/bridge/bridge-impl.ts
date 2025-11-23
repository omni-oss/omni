import { createDeferred, type Deferred } from "@/deferred";
import { TimeoutError, withTimeout } from "@/promise-utils";
import type { Transport } from "@/transport";
import {
    Frame,
    FrameConstants,
    FrameType,
    type StreamEndFrame,
    type StreamStartResponseFrame,
    UnknownFrameSchema,
    type UnknownMessageRequestFrame,
    type UnknownMessageResponseFrame,
    type UnknownStreamDataFrame,
    type UnknownStreamStartFrame,
} from "./frame";
import { Id } from "./id";
import { decode, encode } from "./utils";

type MaybePromise<T> = T | Promise<T>;

export type BridgeRpcConfig = {
    transport: Transport;
    requestHandlers?: Map<string, UnknownBridgeRequestHandler>;
    streamHandlers?: Map<string, UnknownBridgeStreamHandler>;
};

export type RequestContext<TRequestData> = {
    data: TRequestData;
};
export type UnknownBridgeRequestHandler = (
    context: RequestContext<unknown>,
) => MaybePromise<unknown>;
export type BridgeRequestHandler<TRequestData, TResponseData> = (
    data: RequestContext<TRequestData>,
) => MaybePromise<TResponseData>;

export type StreamContext<TStartData, TStreamData> = {
    startData: TStartData;
    stream: AsyncIterable<TStreamData>;
};
export type UnknownBridgeStreamHandler = (
    context: StreamContext<unknown, unknown>,
) => MaybePromise<void>;
export type BridgeStreamHandler<TStartData, TStreamData> = (
    data: StreamContext<TStartData, TStreamData>,
) => MaybePromise<void>;

export class BridgeRpc<
    TRequestHandlers extends [...string[]] = [],
    TStreamHandlers extends [...string[]] = [],
> {
    private responses = new Map<bigint, Deferred<unknown>>();
    private isStarted = false;
    private pendingProbe: Deferred<boolean> | null = null;

    constructor(private readonly config: BridgeRpcConfig) {}

    private async handle(frameBytes: Uint8Array) {
        if (!this.isStarted) {
            return;
        }

        const frame = decode(frameBytes);
        const parsed = UnknownFrameSchema.safeParse(frame);
        if (parsed.success) {
            await this.handleFrame(parsed.data);
        } else {
            await this.respondWithError(
                // biome-ignore lint/suspicious/noExplicitAny: "Allow any for id extraction",
                (frame as any)?.content?.id || Id.create(),
                `invalid frame: ${parsed.error.message}`,
            );
        }
    }

    private async handleFrame(frame: Frame) {
        switch (frame.type) {
            case FrameType.CLOSE: {
                await this.handleClose();
                break;
            }
            case FrameType.CLOSE_ACK: {
                await this.handleCloseAck();
                break;
            }
            case FrameType.PROBE: {
                await this.handleProbe();
                break;
            }
            case FrameType.PROBE_ACK: {
                await this.handleProbeAck();
                break;
            }
            case FrameType.STREAM_START: {
                await this.handleStreamStart(frame);
                break;
            }
            case FrameType.STREAM_START_RESPONSE: {
                await this.handleStreamStartResponse(frame);
                break;
            }
            case FrameType.STREAM_DATA: {
                await this.handleStreamData(frame);
                break;
            }
            case FrameType.STREAM_END: {
                await this.handleStreamEnd(frame);
                break;
            }
            case FrameType.MESSAGE_REQUEST: {
                await this.handleMessageRequest(frame);
                break;
            }
            case FrameType.MESSAGE_RESPONSE: {
                await this.handleMessageResponse(frame);
                break;
            }
        }
    }

    private async sendFrame(frame: Frame) {
        await this.config.transport.send(encode(frame));
    }

    private async handleProbe() {
        await this.sendFrame(FrameConstants.PROBE_ACK);
    }

    private async handleProbeAck() {
        this.pendingProbe?.resolve(true);
        this.pendingProbe = null;
    }

    private async handleClose() {
        await this.sendFrame(FrameConstants.CLOSE_ACK);
        this.responses.clear();
    }

    private async handleCloseAck() {
        this.isStarted = false;
    }

    private async handleStreamStart(_frame: UnknownStreamStartFrame) {}

    private async handleStreamStartResponse(_frame: StreamStartResponseFrame) {}

    private async handleStreamData(_frame: UnknownStreamDataFrame) {}

    private async handleStreamEnd(_frame: StreamEndFrame) {}

    private async respondWithError(id: Id, errorMessage: string) {
        await this.sendFrame(Frame.messageResponseError(id, errorMessage));
    }

    private async handleMessageResponse(response: UnknownMessageResponseFrame) {
        const { id, data, error } = response.data;

        const deferred = this.responses.get(id.getValue());
        if (!deferred) {
            console.warn(`no response handler found for id: ${id}`);
            return;
        }

        this.responses.delete(id.getValue());

        if (error) {
            deferred.reject(new Error(error.message));
        } else {
            deferred.resolve(data);
        }
    }

    private async handleMessageRequest(request: UnknownMessageRequestFrame) {
        const { id, path, data } = request.data;

        try {
            const handler = this.config.requestHandlers?.get(path);
            if (!handler) {
                await this.respondWithError(
                    id,
                    `No handler found for path: ${path}`,
                );
                return;
            }

            const result = await handler({
                data,
            });

            await this.sendFrame(Frame.messageResponseSuccess(id, result));
        } catch (error) {
            await this.respondWithError(
                id,
                `Error handling request for path "${path}": ${error instanceof Error ? error.message : String(error)}`,
            );
        }
    }

    async requestWithId<TResponse>(
        id: Id,
        path: string,
        data: unknown,
    ): Promise<TResponse> {
        const request = Frame.messageRequest(id, path, data);

        const deferred = createDeferred<unknown>();

        this.responses.set(id.getValue(), deferred);

        await this.config.transport.send(encode(request));

        return deferred.promise as Promise<TResponse>;
    }

    async request<TResponse>(path: string, data: unknown): Promise<TResponse> {
        const id = Id.create();
        return await this.requestWithId<TResponse>(id, path, data);
    }

    async stop() {
        if (!this.isStarted) {
            return this;
        }

        this.isStarted = false;
        await this.sendFrame(FrameConstants.CLOSE);
        this.responses.clear();
        return this;
    }

    hasRequestHandler(path: TRequestHandlers[number]): boolean {
        return this.config.requestHandlers?.has(path) ?? false;
    }

    hasStreamHandler(path: TStreamHandlers[number]): boolean {
        return this.config.streamHandlers?.has(path) ?? false;
    }

    private ensureStarted() {
        if (!this.isStarted) {
            throw new Error("RPC is not started");
        }
    }

    async probe(timeoutMs?: number): Promise<boolean> {
        if (this.hasPendingProbe()) {
            throw new Error("Probe already in progress");
        }

        this.ensureStarted();

        const deferred = createDeferred<boolean>();
        this.pendingProbe = deferred;
        await this.sendFrame(FrameConstants.PROBE);

        try {
            return await withTimeout(deferred.promise, timeoutMs ?? 1000);
        } catch (error) {
            if (error instanceof TimeoutError) {
                return false;
            }
            throw error;
        } finally {
            this.pendingProbe = null;
        }
    }

    async start() {
        if (this.isStarted) {
            return this;
        }

        this.isStarted = true;

        this.config.transport.onReceive(this.handle.bind(this));

        return this;
    }

    hasPendingProbe(): boolean {
        return this.pendingProbe !== null;
    }
}
