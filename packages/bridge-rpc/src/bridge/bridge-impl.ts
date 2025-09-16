import { decode, encode } from "@msgpack/msgpack";
import { createDeferred, type Deferred } from "@/deferred";
import { TimeoutError, withTimeout } from "@/promise-utils";
import type { Transport } from "@/transport";
import {
  fClose,
  fProbe,
  fProbeAck,
  fReq,
  fResError,
  fResSuccess,
  type InternalOp,
  type UnknownBridgeFrame,
  UnknownBridgeFrameSchema,
  type UnknownBridgeRequest,
  type UnknownBridgeResponse,
} from "./frame";

type MaybePromise<T> = T | Promise<T>;

export type BridgeRpcConfig = {
  transport: Transport;
  handlers?: Map<string, (arg: unknown) => MaybePromise<unknown>>;
};

function createId(): string {
  return crypto.randomUUID();
}

export type UnknownBridgeRequestHandler = (
  data: unknown,
) => MaybePromise<unknown>;
export type BridgeRequestHandler<TRequest, TResponse> = (
  data: TRequest,
) => MaybePromise<TResponse>;

export class BridgeRpc<THandlers extends [...string[]] = []> {
  private responses = new Map<string, Deferred<unknown>>();
  private isStarted = false;
  private pendingProbe: Deferred<boolean> | null = null;

  constructor(private readonly config: BridgeRpcConfig) {}

  private async handle(frameBytes: Uint8Array) {
    if (!this.isStarted) {
      return;
    }

    const frame = decode(frameBytes);
    const parsed = UnknownBridgeFrameSchema.safeParse(frame);
    if (parsed.success) {
      await this.handleFrame(parsed.data);
    } else {
      await this.respondWithError(
        // biome-ignore lint/suspicious/noExplicitAny: "Allow any for id extraction",
        (frame as any)?.content?.id || createId(),
        `Invalid frame: ${parsed.error.message}`,
      );
    }
  }

  private async handleFrame(frame: UnknownBridgeFrame) {
    switch (frame.type) {
      case "internal_op":
        await this.handleInternalOp(frame.content);
        break;
      case "response":
        this.handleResponse(frame.content);
        break;
      case "request":
        this.handleRequest(frame.content);
        break;
    }
  }

  private async handleInternalOp(op: InternalOp) {
    switch (op) {
      case "close":
        await this.handleClose();
        break;
      case "close_ack":
        await this.handleCloseAck();
        break;
      case "probe":
        await this.handleProbe();
        break;
      case "probe_ack":
        await this.handleProbeAck();
        break;
    }
  }

  private async handleProbe() {
    await this.config.transport.send(encode(fProbeAck()));
  }

  private async handleProbeAck() {
    this.pendingProbe?.resolve(true);
    this.pendingProbe = null;
  }

  private async handleClose() {
    await this.config.transport.send(encode(fClose()));
    this.responses.clear();
  }

  private async handleCloseAck() {
    this.isStarted = false;
  }

  private async respondWithError(id: string, errorMessage: string) {
    await this.config.transport.send(encode(fResError(id, errorMessage)));
  }

  private async handleResponse(response: UnknownBridgeResponse) {
    const { id, data, error } = response;

    const deferred = this.responses.get(id);
    if (!deferred) {
      console.warn(`No response handler found for id: ${id}`);
      return;
    }

    this.responses.delete(id);

    if (error) {
      deferred.reject(new Error(error.error_message));
    } else {
      deferred.resolve(data);
    }
  }

  private async handleRequest(request: UnknownBridgeRequest) {
    const { id, path, data } = request;

    try {
      const handler = this.config.handlers?.get(path);
      if (!handler) {
        await this.respondWithError(id, `No handler found for path: ${path}`);
        return;
      }

      const result = await handler(data);

      await this.config.transport.send(encode(fResSuccess(id, result)));
    } catch (error) {
      await this.respondWithError(
        id,
        `Error handling request for path "${path}": ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }

  async requestWithId<TResponse>(
    id: string,
    path: string,
    data: unknown,
  ): Promise<TResponse> {
    const request = fReq(id, path, data);

    const deferred = createDeferred<unknown>();

    this.responses.set(id, deferred);

    await this.config.transport.send(encode(request));

    return deferred.promise as Promise<TResponse>;
  }

  async request<TResponse>(path: string, data: unknown): Promise<TResponse> {
    const id = createId();
    return await this.requestWithId<TResponse>(id, path, data);
  }

  async stop() {
    this.isStarted = false;
    await this.config.transport.send(encode(fClose()));
    this.responses.clear();
    return this;
  }

  hasHandler(path: THandlers[number]): boolean {
    return this.config.handlers?.has(path) ?? false;
  }

  async probe(timeout?: number): Promise<boolean> {
    if (this.hasPendingProbe()) {
      throw new Error("Probe already in progress");
    }

    if (!this.isStarted) {
      throw new Error("RPC is not started");
    }
    const deferred = createDeferred<boolean>();
    this.pendingProbe = deferred;
    await this.config.transport.send(encode(fProbe()));

    try {
      return await withTimeout(deferred.promise, timeout ?? 1000);
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
