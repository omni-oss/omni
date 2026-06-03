import { Mpsc, Oneshot } from "@omni-oss/channels";
import { describe, expect, it } from "vitest";
import { Id } from "@/id";
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
});

function createPendingRequest(
    path: string,
    pendingResponseFactory?: PendingResponseFactory,
) {
    const id = Id.create();
    const mpsc = new Mpsc<Frame>();
    return {
        id,
        receiver: mpsc.receiver,
        request: new PendingRequest(
            id,
            path,
            mpsc.sender,
            pendingResponseFactory ?? createPendingResponse,
        ),
    };
}

function createActiveRequest(pendingResponseFactory?: PendingResponseFactory) {
    const id = Id.create();
    const mpsc = new Mpsc<Frame>();
    return {
        id,
        receiver: mpsc.receiver,
        request: new ActiveRequest(
            id,
            mpsc.sender,
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
