import { Mpsc, Oneshot } from "@omni-oss/channels";
import { describe, expect, it } from "vitest";
import { Id } from "@/id";
import { ResponseErrorCode } from "../error-code";
import { Frame, type ResponseError, type ResponseStart } from "../frame";
import {
    ActiveRequest,
    PendingRequest,
    type PendingResponseFactory,
} from "./request";
import { PendingResponse, type ResponseFrameEvent } from "./response";

describe("PendingRequest", () => {
    it("should be able to start request", async () => {
        const {
            id,
            request,
            receiver: bytesReceiver,
        } = createPendingRequest("test");

        const headers = {};
        await request.start(headers);

        const frame = await bytesReceiver.receive();

        expect(frame).toBeDefined();
        const expectedFrame = Frame.requestStart(id, "test", headers);
        expect(frame).toEqual(expectedFrame);
    });

    it("should not allow starting request twice", async () => {
        const { request } = createPendingRequest("test");

        await request.start();
        await expect(request.start()).rejects.toThrow(
            "Request already started",
        );
    });

    it("isStarted should reflect whether request is started", async () => {
        const { request } = createPendingRequest("test");

        expect(request.isStarted).toBe(false);

        await request.start();

        expect(request.isStarted).toBe(true);
    });

    it("should throw if error is received while starting", async () => {
        const { request, errorSender } = createPendingRequest("test");

        const error: ResponseError = {
            id: Id.create(),
            code: ResponseErrorCode.UNEXPECTED_FRAME,
            message: "Test error",
        };
        errorSender.send(error);

        await expect(request.start()).rejects.toThrow(
            `Request failed with error code ${error.code.toString()}, ${error.message}`,
        );
    });
});

describe("ActiveRequest", () => {
    it("should be able to send body chunk", async () => {
        const { request, receiver, id } = createActiveRequest();
        const data = new Uint8Array([1, 2, 3]);

        await request.writeBodyChunk(data);

        const frame = await receiver.receive();

        expect(frame).toBeDefined();
        const expectedFrame = Frame.requestBodyChunk(id, data);
        expect(frame).toEqual(expectedFrame);
    });

    it("should return this from writeBodyChunk for chaining", async () => {
        const { request } = createActiveRequest();
        const data1 = new Uint8Array([1, 2, 3]);
        const data2 = new Uint8Array([4, 5, 6]);

        const result = await request
            .writeBodyChunk(data1)
            .then((x) => x.writeBodyChunk(data2));

        expect(result).toBe(request);
    });

    it("should be able to end request", async () => {
        const { request, receiver, id } = createActiveRequest();

        const trailers = {};
        await request.end(trailers);

        const frame = await receiver.receive();

        expect(frame).toBeDefined();
        const expectedFrame = Frame.requestEnd(id, trailers);
        expect(frame).toEqual(expectedFrame);
    });

    it("should not allow writing body chunk after request is ended", async () => {
        const { request } = createActiveRequest();
        await request.end();

        const data = new Uint8Array([1, 2, 3]);
        await expect(request.writeBodyChunk(data)).rejects.toThrow(
            "request is already ended",
        );
    });

    it("should not allow ending request twice", async () => {
        const { request } = createActiveRequest();
        await request.end();

        await expect(request.end()).rejects.toThrow("request is already ended");
    });

    it("isEnded should reflect whether request is ended", async () => {
        const { request } = createActiveRequest();

        expect(request.isEnded).toBe(false);

        await request.end();

        expect(request.isEnded).toBe(true);
    });

    it("should throw if error is received while writing body chunk", async () => {
        const { request, errorSender } = createActiveRequest();

        const error: ResponseError = {
            id: Id.create(),
            code: ResponseErrorCode.UNEXPECTED_FRAME,
            message: "Test error",
        };
        errorSender.send(error);

        const data = new Uint8Array([1, 2, 3]);
        await expect(request.writeBodyChunk(data)).rejects.toThrow(
            `Request failed with error code ${error.code.toString()}, ${error.message}`,
        );
    });

    describe("[Symbol.asyncDispose]", () => {
        it("should end the request when not already ended", async () => {
            const { request, receiver, id } = createActiveRequest();

            expect(request.isEnded).toBe(false);

            await request[Symbol.asyncDispose]();

            expect(request.isEnded).toBe(true);

            const frame = await receiver.receive();
            expect(frame).toEqual(Frame.requestEnd(id, undefined));
        });

        it("should be a no-op when the request is already ended", async () => {
            const { request, receiver, id } = createActiveRequest();

            const trailers = { foo: "bar" };
            await request.end(trailers);

            // Drain the end frame produced by end()
            const endFrame = await receiver.receive();
            expect(endFrame).toEqual(Frame.requestEnd(id, trailers));

            // Should not throw and should not send another frame
            await expect(
                request[Symbol.asyncDispose](),
            ).resolves.toBeUndefined();

            expect(request.isEnded).toBe(true);
        });

        it("should work with `await using` syntax", async () => {
            const { receiver, id } = createActiveRequest();

            // Re-create within the block so we can use `await using`
            const innerMpsc = new Mpsc<Frame>();
            const error = new Oneshot<ResponseError>();
            const innerId = id;
            {
                await using request = new ActiveRequest(
                    innerId,
                    innerMpsc.sender,
                    error.receiver,
                    createPendingResponse,
                );
                expect(request.isEnded).toBe(false);
            }

            const frame = await innerMpsc.receiver.receive();
            expect(frame).toEqual(Frame.requestEnd(innerId, undefined));

            // ensure we don't have lingering frames in the original receiver
            void receiver;
        });
    });
});

function createPendingRequest(
    path: string,
    pendingResponseFactory?: PendingResponseFactory,
) {
    const id = Id.create();
    const mpsc = new Mpsc<Frame>();
    const error = new Oneshot<ResponseError>();
    return {
        id,
        receiver: mpsc.receiver,
        errorSender: error.sender,
        request: new PendingRequest(
            id,
            path,
            mpsc.sender,
            error.receiver,
            pendingResponseFactory ?? createPendingResponse,
        ),
    };
}

function createActiveRequest(pendingResponseFactory?: PendingResponseFactory) {
    const id = Id.create();
    const mpsc = new Mpsc<Frame>();
    const error = new Oneshot<ResponseError>();
    return {
        id,
        receiver: mpsc.receiver,
        errorSender: error.sender,
        request: new ActiveRequest(
            id,
            mpsc.sender,
            error.receiver,
            pendingResponseFactory ?? createPendingResponse,
        ),
    };
}

function createPendingResponse(id: Id) {
    const responseStartReceiver = new Oneshot<ResponseStart>();
    const responseFrameReceiver = new Mpsc<ResponseFrameEvent>();
    const responseErrorReceiver = new Oneshot<ResponseError>();

    return new PendingResponse(
        id,
        responseStartReceiver.receiver,
        responseFrameReceiver.receiver,
        responseErrorReceiver.receiver,
    );
}
