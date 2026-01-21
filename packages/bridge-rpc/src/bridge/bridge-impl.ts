import { Mutex } from "async-mutex";
import { Deferred } from "@/deferred";
import { Id } from "@/id";
import { Mpsc, type MpscReceiver } from "@/mpsc";
import { Oneshot, type OneshotReceiver } from "@/oneshot";
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
import { decode, encode } from "./codec-utils";
import { RequestSessionContext, ResponseSessionContext } from "./contexts";
import type { Headers } from "./dyn-map";
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
    type ResponseEvent,
    ResponseEventType,
    SessionManager,
} from "./session";
import type { ResponseStatusCode } from "./status-code";

export class BridgeRpc {
    private sessionManager = new SessionManager<
        RequestSessionContext,
        ResponseSessionContext
    >();
    private isStarted = false;
    private pendingPing: Deferred<void> | undefined = undefined;
    private frameTransporter: FrameTransporter;
    private serviceTaskBackgroundProcessor = new BackgroundProcessor();
    private id: Id = Id.create();
    private mutex = new Mutex();

    constructor(
        private transport: Transport,
        private service: Service,
    ) {
        this.frameTransporter = new FrameTransporter((bytes) =>
            transport.send(bytes),
        );
    }

    public async requestWithId(id: Id, path: string) {
        this.ensureStarted();

        const responseSession = this.startResponseSession(id);

        const request = new PendingRequest(
            id,
            path,
            this.frameTransporter.sender,
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

    public request(path: string) {
        return this.requestWithId(Id.create(), path);
    }

    public async start() {
        return await this.runExclusive(async () => {
            if (this.isStarted) {
                console.info(`rpc ${this.id} is already started`);
                return;
            }

            this.transport.onReceive(this.handle.bind(this));
            await this.frameTransporter.start();
            this.isStarted = true;
            console.info(`rpc ${this.id} started`);
        });
    }

    public async stop() {
        return await this.runExclusive(async () => {
            if (!this.isStarted) {
                return;
            }

            await this.sendFrame(Frame.close());
            await this.frameTransporter.stop();
            await this.serviceTaskBackgroundProcessor.awaitAll();
            console.info(`rpc ${this.id} stopped`);
            this.isStarted = false;
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

        const frame = decode(frameBytes);
        const parsed = FrameSchema.safeParse(frame);
        if (parsed.success) {
            await this.handleFrame(parsed.data);
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
        console.error(
            `invalid frame received (frame type: ${(frame as Frame).type}): ${errorMessage}`,
        );
    }

    private async handleFrame(frame: Frame) {
        let event: Event;

        console.log(
            `[rpc_id: ${this.id}]: received frame: ${frame.type} with id: ${frame.data?.id}`,
        );

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
                console.error(
                    `unsupported frame type: ${(frame as unknown as { type: number }).type}`,
                );
                return;
        }

        switch (event.type) {
            case EventType.REQUEST:
                {
                    const requestSession =
                        event.event.type === RequestEventType.START
                            ? this.startRequestSession(event.event.data.id)
                            : {
                                  session: this.getRequestSession(
                                      event.event.data.id,
                                  ),
                                  requestFrameReceiver: undefined,
                                  requestErrorReceiver: undefined,
                              };

                    if (!requestSession.session) {
                        throw new Error(
                            `cannot find request session for id: ${event.event.data.id}, this should never happen, please report this bug`,
                        );
                    }

                    const { output, context } =
                        await requestSession.session.runExclusive(
                            (session) => ({
                                output: session.state.transition(event.event),
                                context: session.context,
                            }),
                        );

                    switch (output.type) {
                        case "Start":
                            if (
                                requestSession.requestFrameReceiver ===
                                    undefined ||
                                requestSession.requestErrorReceiver ===
                                    undefined
                            ) {
                                throw new Error(
                                    `no request frame receiver or error receiver found for request session with id: ${event.event.data.id}, this should never happen, please report this bug`,
                                );
                            }

                            await this.handleRequestStart(
                                event.event.data.id,
                                output.path,
                                output.headers,
                                // biome-ignore lint/style/noNonNullAssertion: should have value here
                                requestSession.requestFrameReceiver!,
                                // biome-ignore lint/style/noNonNullAssertion: should have value here
                                requestSession.requestErrorReceiver!,
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
                            throw new Error("invalid request event type");
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
                        throw new Error(
                            `cannot find response session for id: ${event.event.data.id}, this should never happen, please report this bug`,
                        );
                    }

                    const { output, context } =
                        await responseSession.runExclusive((session) => ({
                            output: session.state.transition(event.event),
                            context: session.context,
                        }));
                    switch (output.type) {
                        case "Start":
                            await this.handleResponseStart(
                                context,
                                output.id,
                                output.status,
                                output.headers,
                            );
                            break;
                        case "BodyChunk":
                            await this.handleResponseBodyChunk(
                                context,
                                output.chunk,
                            );
                            break;
                        case "End":
                            await this.handleResponseEnd(
                                context,
                                output.trailers,
                            );
                            break;
                        case "Error":
                            await this.handleResponseError(
                                context,
                                output.error,
                            );
                            break;
                        case "Wait":
                            // do nothing
                            break;
                        default:
                            throw new Error("invalid response event type");
                    }

                    if (output.type === "End" || output.type === "Error") {
                        await this.closeResponseSession(event.event.data.id);
                    }
                }
                break;
            default:
                throw new Error("invalid event type");
        }
    }

    private startResponseSession(id: Id) {
        console.info(
            `[rpc_id: ${this.id}]: starting response session with id: ${id}`,
        );
        const responseStart = new Oneshot<ResponseStart>();
        const responseFrame = new Mpsc<ResponseFrameEvent>();
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
        console.info(
            `[rpc_id: ${this.id}]: started response session with id: ${id}`,
        );
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
        console.info(
            `[rpc_id: ${this.id}]: starting request session with id: ${id}`,
        );
        const requestFrame = new Mpsc<RequestFrameEvent>();
        const requestError = new Oneshot<RequestError>();
        const requestSessionContext = new RequestSessionContext(
            requestFrame.sender,
            requestError.sender,
        );

        const session = this.sessionManager.startRequestSession(
            id,
            requestSessionContext,
        );

        console.info(
            `[rpc_id: ${this.id}]: started request session with id: ${id}`,
        );

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

        const serviceContext = new ServiceContext(request, response);

        this.serviceTaskBackgroundProcessor.queue(
            this.service.run(serviceContext),
        );
    }

    private async handleRequestBodyChunk(
        context: RequestSessionContext,
        chunk: Uint8Array,
    ) {
        context.requestFrameSender.send({
            type: RequestFrameEventType.BODY_CHUNK,
            chunk,
        });
    }

    private async handleRequestEnd(
        context: RequestSessionContext,
        trailers: Headers | undefined,
    ) {
        context.requestFrameSender.send({
            type: RequestFrameEventType.END,
            trailers,
        });
    }

    private async handleRequestError(
        context: RequestSessionContext,
        error: RequestError,
    ) {
        context.requestErrorSender.send(error);
    }

    private async handleResponseStart(
        context: ResponseSessionContext,
        id: Id,
        status: ResponseStatusCode,
        headers: Headers | undefined,
    ) {
        context.responseStartSender.send({
            id,
            status,
            headers,
        });
    }

    private async handleResponseBodyChunk(
        context: ResponseSessionContext,
        chunk: Uint8Array,
    ) {
        context.responseFrameSender.send({
            type: ResponseFrameEventType.BODY_CHUNK,
            chunk,
        });
    }

    private async handleResponseEnd(
        context: ResponseSessionContext,
        trailers: Headers | undefined,
    ) {
        context.responseFrameSender.send({
            type: ResponseFrameEventType.END,
            trailers,
        });
    }

    private async handleResponseError(
        context: ResponseSessionContext,
        error: ResponseError,
    ) {
        context.responseErrorSender.send(error);
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

    private async sendFrame(frame: Frame) {
        await this.transport.send(encode(frame));
    }

    private runExclusive<T>(fn: () => T): Promise<T> {
        return this.mutex.runExclusive(fn);
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
