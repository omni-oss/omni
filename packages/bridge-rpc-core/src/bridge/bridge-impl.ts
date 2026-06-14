import { Deferred } from "@omni-oss/async-utils";
import {
    Mpsc,
    type MpscReceiver,
    Oneshot,
    type OneshotReceiver,
} from "@omni-oss/channels";
import { Log, type Logger } from "@omni-oss/log";
import { Mutex } from "async-mutex";
import { Id } from "@/id";
import { TimeoutError, withTimeout } from "@/promise-utils";
import type { Transport } from "@/transport";
import { BackgroundProcessor } from "./background-processor";
import {
    PendingResponse as ClientPendingResponse,
    PendingRequest,
} from "./client";
import {
    type ResponseFrameEvent,
    ResponseFrameEventType,
} from "./client/response";
import { ClientHandle } from "./client-handle";
import { decodeFrame, encodeFrame } from "./codec-utils";
import { RESPONSE_BUFFER_SIZE } from "./constants";
import { RequestSessionContext, ResponseSessionContext } from "./contexts";
import type { Headers } from "./dyn-map";
import { ResponseErrorCode } from "./error-code";
import {
    Frame,
    type RequestError,
    type ResponseError,
    type ResponseStart,
} from "./frame";
import { FrameSchema, FrameType } from "./frame-schema";
import { FrameTransporter } from "./frame-transporter";
import {
    Request,
    type RequestFrameEvent,
    RequestFrameEventType,
} from "./server/request";
import { PendingResponse } from "./server/response";
import { type Service, ServiceContext } from "./service";
import {
    type RequestEvent,
    RequestEventType,
    type RequestStateTransitionOutput,
    type ResponseEvent,
    ResponseEventType,
    type ResponseStateTransitionOutput,
    SessionManager,
} from "./session";
import type { ResponseStatusCode } from "./status-code";

export class BridgeRpc {
    private sessionManager = new SessionManager<
        RequestSessionContext,
        ResponseSessionContext
    >({
        get logger() {
            return BridgeRpc._logger;
        },
    });
    private isStarted = false;
    private pendingPing: Deferred<void> | undefined = undefined;
    private frameTransporter: FrameTransporter;
    private serviceTaskBackgroundProcessor = new BackgroundProcessor();
    private mutex = new Mutex();
    private _clientHandle: ClientHandle;
    private static _logger: Logger | undefined = undefined;

    constructor(
        private transport: Transport,
        private service: Service,
    ) {
        this.frameTransporter = new FrameTransporter((bytes) =>
            transport.send(bytes),
        );

        const requestWithId = this.requestWithId.bind(this);
        const request = this.request.bind(this);

        this._clientHandle = new (class extends ClientHandle {
            requestWithId = requestWithId;
            request = request;
        })();
    }

    public async requestWithId(id: Id, path: string) {
        this.ensureStarted();

        const responseSession = this.startResponseSession(id);

        const request = new PendingRequest(
            id,
            path,
            this.frameTransporter.sender,
            responseSession.responseErrorReceiver,
            (id) =>
                new ClientPendingResponse(
                    id,
                    responseSession.responseStartReceiver,
                    responseSession.responseFrameReceiver,
                    responseSession.responseErrorReceiver,
                ),
        );

        return request;
    }

    public get clientHandle(): ClientHandle {
        return this._clientHandle;
    }

    public request(path: string) {
        return this.requestWithId(Id.create(), path);
    }

    public async start() {
        return await this.runExclusive(async () => {
            if (this.isStarted) {
                this.logger.warn(`rpc is already started`);
                return;
            }

            this.transport.onReceive(this.handle.bind(this));
            await this.frameTransporter.start();
            this.isStarted = true;
            this.logger.trace("rpc started");
        });
    }

    public async stop() {
        return await this.runExclusive(async () => {
            if (!this.isStarted) {
                return;
            }

            try {
                await this.frameTransporter.stop();
                await this.serviceTaskBackgroundProcessor.awaitAll();
            } finally {
                await this.sendFrame(Frame.close());
                this.logger.trace("rpc stoppped");
                this.isStarted = false;
            }
        });
    }

    public async ping(timeoutMs: number) {
        return await this.runExclusive(async () => {
            this.ensureStarted();

            if (this.pendingPing) {
                throw new Error("pending ping is already in progress");
            }

            const deferred = new Deferred<void>();
            this.pendingPing = deferred;

            await this.sendFrame(Frame.ping());

            try {
                await withTimeout(deferred.promise, timeoutMs);
                return true;
            } catch (e) {
                if (e instanceof TimeoutError) {
                    return false;
                } else {
                    throw e;
                }
            } finally {
                this.pendingPing = undefined;
            }
        });
    }

    private async handle(frameBytes: Uint8Array) {
        this.ensureStarted();

        const frame = decodeFrame(frameBytes);
        const parsed = FrameSchema.safeParse(frame);
        if (parsed.success) {
            try {
                await this.handleFrame(parsed.data);
            } catch (error) {
                this.logger.error(
                    `unexpected error while handling frame: ${error}`,
                );
            }
        } else {
            await this.handleInvalidFrame(frame as Frame, parsed.error.message);
        }
    }

    private ensureStarted() {
        if (!this.isStarted) {
            throw new Error("RPC is not started");
        }
    }

    private async handleInvalidFrame(frame: unknown, errorMessage: string) {
        this.logger.error(
            `invalid frame received (frame type: ${(frame as Frame).type}): ${errorMessage}`,
        );
    }

    private async handleFrame(frame: Frame) {
        let event: Event;

        this.logger.trace(`received frame of type: ${frame.type}`);

        switch (frame.type) {
            case FrameType.REQUEST_START:
                event = makeRequestEvent(RequestEventType.START, frame);
                break;

            case FrameType.REQUEST_BODY_CHUNK:
                event = makeRequestEvent(RequestEventType.BODY_CHUNK, frame);
                break;

            case FrameType.REQUEST_END:
                event = makeRequestEvent(RequestEventType.END, frame);
                break;

            case FrameType.REQUEST_ERROR:
                event = makeRequestEvent(RequestEventType.ERROR, frame);
                break;

            case FrameType.RESPONSE_START:
                event = makeResponseEvent(ResponseEventType.START, frame);
                break;

            case FrameType.RESPONSE_BODY_CHUNK:
                event = makeResponseEvent(ResponseEventType.BODY_CHUNK, frame);
                break;

            case FrameType.RESPONSE_END:
                event = makeResponseEvent(ResponseEventType.END, frame);
                break;

            case FrameType.RESPONSE_ERROR:
                event = makeResponseEvent(ResponseEventType.ERROR, frame);
                break;

            case FrameType.CLOSE:
                await this.handleClose();
                return;

            case FrameType.PING:
                await this.handlePing();
                return;

            case FrameType.PONG:
                await this.handlePong();
                return;

            default:
                this.logger.error(
                    `unsupported frame type: ${(frame as unknown as { type: number }).type}`,
                );
                return;
        }

        switch (event.type) {
            case EventType.REQUEST:
                {
                    // For START events: create new session with frame receivers
                    // For other events: look up the existing session mutex directly
                    let sessionResult:
                        | ReturnType<typeof this.startRequestSession>
                        | undefined;

                    if (event.event.type === RequestEventType.START) {
                        try {
                            sessionResult = this.startRequestSession(
                                event.event.data.id,
                            );
                        } catch (e) {
                            this.logger.warn(
                                `failed to start request session for id ${event.event.data.id}: ${e}`,
                            );
                            await this.sendResponseErrorFrame(
                                event.event.data.id,
                                ResponseErrorCode.UNEXPECTED_FRAME,
                                `cannot start session: ${e}`,
                            );
                            break;
                        }
                    }

                    const sessionMutex =
                        sessionResult?.session ??
                        this.getRequestSession(event.event.data.id);

                    if (!sessionMutex) {
                        this.logger.warn(
                            `no request session found for id: ${event.event.data.id}`,
                        );
                        await this.sendResponseErrorFrame(
                            event.event.data.id,
                            ResponseErrorCode.UNEXPECTED_FRAME,
                            `no request session for id: ${event.event.data.id}`,
                        );
                        break;
                    }

                    let result:
                        | {
                              output: RequestStateTransitionOutput;
                              context: RequestSessionContext;
                          }
                        | undefined;
                    try {
                        result = await sessionMutex.runExclusive((session) => ({
                            output: session.state.transition(event.event),
                            context: session.context,
                        }));
                    } catch (e) {
                        this.logger.warn(
                            `request state machine error for id ${event.event.data.id}: ${e}`,
                        );
                        await this.sendResponseErrorFrame(
                            event.event.data.id,
                            ResponseErrorCode.UNEXPECTED_FRAME,
                            `protocol error: ${e}`,
                        );
                        await this.closeRequestSession(event.event.data.id);
                        break;
                    }

                    // biome-ignore lint/style/noNonNullAssertion: should be appropriate to assert given the checks above and the state machine contract
                    const { output, context } = result!;

                    switch (output.type) {
                        case "Start":
                            if (!sessionResult) {
                                this.logger.error(
                                    `no request frame receiver or error receiver found for request session with id: ${event.event.data.id}`,
                                );
                                break;
                            }

                            await this.handleRequestStart(
                                event.event.data.id,
                                output.path,
                                output.headers,
                                sessionResult.requestFrameReceiver,
                                sessionResult.requestErrorReceiver,
                            );
                            break;
                        case "BodyChunk":
                            await this.handleRequestBodyChunk(
                                context,
                                output.chunk,
                            );
                            break;
                        case "End":
                            await this.handleRequestEnd(
                                context,
                                output.trailers,
                            );
                            break;
                        case "Error":
                            await this.handleRequestError(
                                context,
                                output.error,
                            );
                            break;
                        case "Wait":
                            // do nothing
                            break;
                        default:
                            this.logger.error(
                                `unexpected request transition output type`,
                            );
                    }

                    if (output.type === "End" || output.type === "Error") {
                        await this.closeRequestSession(event.event.data.id);
                    }
                }
                break;
            case EventType.RESPONSE:
                {
                    const responseSession = this.getResponseSession(
                        event.event.data.id,
                    );

                    if (!responseSession) {
                        this.logger.warn(
                            `no response session found for id: ${event.event.data.id}, ignoring`,
                        );
                        break;
                    }

                    let responseResult:
                        | {
                              output: ResponseStateTransitionOutput;
                              context: ResponseSessionContext;
                          }
                        | undefined;
                    try {
                        responseResult = await responseSession.runExclusive(
                            (session) => ({
                                output: session.state.transition(event.event),
                                context: session.context,
                            }),
                        );
                    } catch (e) {
                        this.logger.warn(
                            `response state machine error for id ${event.event.data.id}: ${e}`,
                        );
                        await this.closeResponseSession(event.event.data.id);
                        break;
                    }

                    const { output: responseOutput, context: responseContext } =
                        // biome-ignore lint/style/noNonNullAssertion: should be appropriate to assert given the checks above and the state machine contract
                        responseResult!;

                    switch (responseOutput.type) {
                        case "Start":
                            await this.handleResponseStart(
                                responseContext,
                                responseOutput.id,
                                responseOutput.status,
                                responseOutput.headers,
                            );
                            break;
                        case "BodyChunk":
                            await this.handleResponseBodyChunk(
                                responseContext,
                                responseOutput.chunk,
                            );
                            break;
                        case "End":
                            await this.handleResponseEnd(
                                responseContext,
                                responseOutput.trailers,
                            );
                            break;
                        case "Error":
                            await this.handleResponseError(
                                responseContext,
                                responseOutput.error,
                            );
                            break;
                        case "Wait":
                            // do nothing
                            break;
                        default:
                            this.logger.error(
                                `unexpected response transition output type`,
                            );
                    }

                    if (
                        responseOutput.type === "End" ||
                        responseOutput.type === "Error"
                    ) {
                        await this.closeResponseSession(event.event.data.id);
                    }
                }
                break;
            default:
                this.logger.error(`unexpected event type`);
        }
    }

    private startResponseSession(id: Id) {
        this.logger.trace("starting response session with id: {id}", {
            id,
        });
        const responseStart = new Oneshot<ResponseStart>();
        const responseFrame = new Mpsc<ResponseFrameEvent, number>(
            RESPONSE_BUFFER_SIZE,
        );
        const responseError = new Oneshot<ResponseError>();
        const responseSessionContext = new ResponseSessionContext(
            responseStart.sender,
            responseFrame.sender,
            responseError.sender,
        );
        const session = this.sessionManager.startResponseSession(
            id,
            responseSessionContext,
        );
        this.logger.trace("started response session with id: {id}", {
            id,
        });
        return {
            session,
            responseStartReceiver: responseStart.receiver,
            responseFrameReceiver: responseFrame.receiver,
            responseErrorReceiver: responseError.receiver,
        };
    }

    private getRequestSession(id: Id) {
        return this.sessionManager.getRequestSession(id);
    }

    private closeRequestSession(id: Id) {
        return this.sessionManager.closeRequestSession(id);
    }

    private getResponseSession(id: Id) {
        return this.sessionManager.getResponseSession(id);
    }

    private closeResponseSession(id: Id) {
        return this.sessionManager.closeResponseSession(id);
    }

    private startRequestSession(id: Id) {
        this.logger.trace("starting request session with id: {id}", { id });
        const requestFrame = new Mpsc<RequestFrameEvent, number>(
            RESPONSE_BUFFER_SIZE,
        );
        const requestError = new Oneshot<RequestError>();
        const requestSessionContext = new RequestSessionContext(
            requestFrame.sender,
            requestError.sender,
        );

        const session = this.sessionManager.startRequestSession(
            id,
            requestSessionContext,
        );

        this.logger.trace("started request session with id: {id}", { id });

        return {
            session,
            requestFrameReceiver: requestFrame.receiver,
            requestErrorReceiver: requestError.receiver,
        };
    }

    private async handleRequestStart(
        id: Id,
        path: string,
        headers: Headers | undefined,
        requestFrameReceiver: MpscReceiver<RequestFrameEvent>,
        requestErrorReceiver: OneshotReceiver<RequestError>,
    ) {
        const request = new Request(
            id,
            path,
            headers,
            requestFrameReceiver,
            requestErrorReceiver,
        );

        const response = new PendingResponse(id, this.frameTransporter.sender);

        const serviceContext = new ServiceContext(
            request,
            response,
            this._clientHandle,
        );

        this.serviceTaskBackgroundProcessor.queue(
            this.service.run(serviceContext).catch(async (error) => {
                // Let the requesting client know the service failed by
                // propagating the error back over the wire as a response
                // error frame. Once it has been reported to the client the
                // error is considered handled, so it is only logged here and
                // not propagated any further.
                this.logger.error(`service error for request ${id}: ${error}`);
                await this.sendResponseErrorFrame(
                    id,
                    ResponseErrorCode.INTERNAL,
                    error instanceof Error ? error.message : String(error),
                );
            }),
        );
    }

    private async handleRequestBodyChunk(
        context: RequestSessionContext,
        chunk: Uint8Array,
    ) {
        // Awaiting applies backpressure: if the handler is slow to drain the
        // request body, the receive loop pauses instead of buffering without
        // bound. A closed channel means the consumer is gone, so the chunk is
        // dropped (mirrors Rust logging and continuing).
        try {
            await context.requestFrameSender.send({
                type: RequestFrameEventType.BODY_CHUNK,
                chunk,
            });
        } catch (e) {
            this.logger.warn(`failed to forward request body chunk: ${e}`);
        }
    }

    private async handleRequestEnd(
        context: RequestSessionContext,
        trailers: Headers | undefined,
    ) {
        try {
            await context.requestFrameSender.send({
                type: RequestFrameEventType.END,
                trailers,
            });
        } catch (e) {
            this.logger.warn(`failed to forward request end: ${e}`);
        }
    }

    private async handleRequestError(
        context: RequestSessionContext,
        error: RequestError,
    ) {
        this.logger.warn(`request error: ${error.message}`, { cause: error });
        try {
            context.requestErrorSender.send(error);
        } catch (e) {
            this.logger.warn(`failed to send request error: ${e}`);
        }
    }

    private async handleResponseStart(
        context: ResponseSessionContext,
        id: Id,
        status: ResponseStatusCode,
        headers: Headers | undefined,
    ) {
        try {
            context.responseStartSender.send({
                id,
                status,
                headers,
            });
        } catch (e) {
            this.logger.warn(`failed to send response start: ${e}`);
        }
    }

    private async handleResponseBodyChunk(
        context: ResponseSessionContext,
        chunk: Uint8Array,
    ) {
        try {
            await context.responseFrameSender.send({
                type: ResponseFrameEventType.BODY_CHUNK,
                chunk,
            });
        } catch (e) {
            this.logger.warn(`failed to forward response body chunk: ${e}`);
        }
    }

    private async handleResponseEnd(
        context: ResponseSessionContext,
        trailers: Headers | undefined,
    ) {
        try {
            await context.responseFrameSender.send({
                type: ResponseFrameEventType.END,
                trailers,
            });
        } catch (e) {
            this.logger.warn(`failed to forward response end: ${e}`);
        }
    }

    private async handleResponseError(
        context: ResponseSessionContext,
        error: ResponseError,
    ) {
        this.logger.warn(`response error: ${error.message}`, { cause: error });
        try {
            context.responseErrorSender.send(error);
        } catch (e) {
            this.logger.warn(`failed to send response error: ${e}`);
        }
    }

    private async handleClose() {
        this.isStarted = false;
    }

    private async handlePing() {
        await this.sendFrame(Frame.pong());
    }

    private async handlePong() {
        if (!this.pendingPing) {
            return;
        } else {
            this.pendingPing.resolve(undefined);
        }
    }

    private async sendResponseErrorFrame(
        id: Id,
        code: ResponseErrorCode,
        message: string,
    ) {
        const frame = Frame.responseError(id, code, message);
        // Fire-and-forget: error frames must not block the receive loop on the
        // bounded worker channel, and a closed transport simply means there is
        // nothing to notify.
        try {
            await this.frameTransporter.sender.send(frame).catch((e) => {
                this.logger.warn(`failed to send response error frame: ${e}`);
            });
        } catch (e) {
            this.logger.warn(`failed to send response error frame: ${e}`);
        }
    }

    private sendFrame(frame: Frame) {
        return this.transport.send(encodeFrame(frame));
    }

    private runExclusive<T>(fn: () => T): Promise<T> {
        return this.mutex.runExclusive(fn);
    }

    private get logger(): Logger {
        if (!BridgeRpc._logger) {
            BridgeRpc._logger = Log.get("BridgeRpc");
        }

        return BridgeRpc._logger;
    }
}

enum EventType {
    REQUEST = 0,
    RESPONSE = 1,
}

type Event =
    | {
          type: EventType.REQUEST;
          event: RequestEvent;
      }
    | { type: EventType.RESPONSE; event: ResponseEvent };

function makeRequestEvent(eventType: RequestEventType, frame: Frame): Event {
    if (
        !(
            frame.type === FrameType.REQUEST_START ||
            frame.type === FrameType.REQUEST_BODY_CHUNK ||
            frame.type === FrameType.REQUEST_END ||
            frame.type === FrameType.REQUEST_ERROR
        )
    ) {
        throw new Error("invalid request frame");
    }

    return {
        type: EventType.REQUEST,
        event: {
            type: eventType,
            data: frame.data,
        } as unknown as RequestEvent,
    };
}

function makeResponseEvent(eventType: ResponseEventType, frame: Frame): Event {
    if (
        !(
            frame.type === FrameType.RESPONSE_START ||
            frame.type === FrameType.RESPONSE_BODY_CHUNK ||
            frame.type === FrameType.RESPONSE_END ||
            frame.type === FrameType.RESPONSE_ERROR
        )
    ) {
        throw new Error("invalid response frame");
    }

    return {
        type: EventType.RESPONSE,
        event: {
            type: eventType,
            data: frame.data,
        } as unknown as ResponseEvent,
    };
}
