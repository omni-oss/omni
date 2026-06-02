import { Mpsc } from "@omni-oss/channels";
import { describe, expect, it } from "vitest";
import { Id } from "@/id";
import { Frame } from "../frame";
import { ResponseStatusCode } from "../status-code";
import { ActiveResponse, PendingResponse } from "./response";

describe("PendingResponse", () => {
    it("should be able to start response", async () => {
        const { id, request, receiver } = createPendingResponse();

        const headers = {};
        await request.start(ResponseStatusCode.SUCCESS, headers);

        const frame = await receiver.receive();

        expect(frame).toBeDefined();
        const expectedFrame = Frame.responseStart(
            id,
            ResponseStatusCode.SUCCESS,
            headers,
        );
        expect(frame).toEqual(expectedFrame);
    });

    it("should not allow starting response twice", async () => {
        const { request } = createPendingResponse();

        await request.start(ResponseStatusCode.SUCCESS);
        await expect(request.start(ResponseStatusCode.SUCCESS)).rejects.toThrow(
            "Response already started",
        );
    });

    it("isStarted should reflect whether response is started", async () => {
        const { request } = createPendingResponse();

        expect(request.isStarted).toBe(false);

        await request.start(ResponseStatusCode.SUCCESS);

        expect(request.isStarted).toBe(true);
    });
});

describe("ActiveResponse", () => {
    it("should be able to send body chunk", async () => {
        const { request, receiver, id } = createActiveResponse();
        const data = new Uint8Array([1, 2, 3]);

        await request.writeBodyChunk(data);

        const frame = await receiver.receive();
        expect(frame).toBeDefined();
        const expectedFrame = Frame.responseBodyChunk(id, data);
        expect(frame).toEqual(expectedFrame);
    });

    it("should be able to end response", async () => {
        const { request, receiver, id } = createActiveResponse();

        const trailers = {};
        await request.end(trailers);

        const frame = await receiver.receive();

        expect(frame).toBeDefined();
        const expectedFrame = Frame.responseEnd(id, trailers);
        expect(frame).toEqual(expectedFrame);
    });

    it("should not allow writing body chunk after response is ended", async () => {
        const { request } = createActiveResponse();
        await request.end();

        const data = new Uint8Array([1, 2, 3]);
        await expect(request.writeBodyChunk(data)).rejects.toThrow(
            "Response already ended",
        );
    });

    it("should not allow ending response twice", async () => {
        const { request } = createActiveResponse();
        await request.end();

        await expect(request.end()).rejects.toThrow("Response already ended");
    });

    it("isEnded should reflect whether response is ended", async () => {
        const { request } = createActiveResponse();

        expect(request.isEnded).toBe(false);

        await request.end();

        expect(request.isEnded).toBe(true);
    });
});

function createPendingResponse() {
    const id = Id.create();
    const mpsc = new Mpsc<Frame>();
    return {
        id,
        receiver: mpsc.receiver,
        request: new PendingResponse(id, mpsc.sender),
    };
}

function createActiveResponse() {
    const id = Id.create();
    const mpsc = new Mpsc<Frame>();
    return {
        id,
        receiver: mpsc.receiver,
        request: new ActiveResponse(id, mpsc.sender),
    };
}
