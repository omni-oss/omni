import { describe, expect, it } from "vitest";
import { Id } from "@/id";
import { Mpsc } from "@/mpsc";
import { Oneshot } from "@/oneshot";
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

    it("should be able to end request", async () => {
        const { request, receiver, id } = createActiveRequest();

        const trailers = {};
        await request.end(trailers);

        const frame = await receiver.receive();

        expect(frame).toBeDefined();
        const expectedFrame = Frame.requestEnd(id, trailers);
        expect(frame).toEqual(expectedFrame);
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
