import { Mpsc, Oneshot } from "@omni-oss/channels";
import { describe, expect, it } from "vitest";
import { Id } from "@/id";
import { readAll } from "../byte-array-utils";
import type { Headers } from "../dyn-map";
import { ResponseErrorCode } from "../error-code";
import type { ResponseError, ResponseStart } from "../frame";
import { ResponseStatusCode } from "../status-code";
import {
    PendingResponse,
    type ResponseFrameEvent,
    ResponseFrameEventType,
} from "./response";

describe("PendingResponse", () => {
    it("should be able to wait for response", async () => {
        const pendingResponse = createPendingResponse(
            {
                status: ResponseStatusCode.SUCCESS,
                headers: {},
            },
            {
                chunks: [new Uint8Array([1, 2, 3])],
                trailers: {},
            },
        );

        const response = await pendingResponse.response.wait();

        expect(response).toBeDefined();
        expect(response.status).toBe(ResponseStatusCode.SUCCESS);
        expect(response.headers).toEqual({});
    });
});

describe("Respnse", () => {
    it("should be able to read body chunk", async () => {
        const data = new Uint8Array([1, 2, 3]);
        const { response: pendingResponse } = createPendingResponse(
            {
                status: ResponseStatusCode.SUCCESS,
                headers: {},
            },
            {
                chunks: [data],
                trailers: {},
            },
        );
        const response = await pendingResponse.wait();

        const bytes = await readAll(response.readBody());

        expect(bytes).toBeDefined();
        expect(bytes).toEqual(data);
    });

    it("should be able to receive trailers", async () => {
        const trailers = { test: 1232 };
        const { response: pendingResponse } = createPendingResponse(
            {
                status: ResponseStatusCode.SUCCESS,
                headers: {},
            },
            {
                chunks: [new Uint8Array([1, 2, 3])],
                trailers,
            },
        );

        const response = await pendingResponse.wait();
        const _1 = await readAll(response.readBody());

        expect(response.trailers).toBeDefined();
        expect(response.trailers).toEqual(trailers);
    });

    it("should throw error when reading body chunk after end", async () => {
        const { response: pendingResponse } = createPendingResponse(
            {
                status: ResponseStatusCode.SUCCESS,
                headers: {},
            },
            {
                chunks: [new Uint8Array([1, 2, 3])],
            },
        );
        const response = await pendingResponse.wait();

        await readAll(response.readBody());

        await expect(readAll(response.readBody())).rejects.toThrowError();
    });

    it("should throw error when received error frame", async () => {
        const {
            id,
            response: pendingResponse,
            responseErrorSender,
        } = createPendingResponse(
            {
                status: ResponseStatusCode.SUCCESS,
                headers: {},
            },
            {
                chunks: [new Uint8Array([1, 2, 3])],
                trailers: {},
            },
        );

        const response = await pendingResponse.wait();

        responseErrorSender.send({
            code: ResponseErrorCode.UNEXPECTED_FRAME,
            id,
            message: "test",
        });

        await expect(readAll(response.readBody())).rejects.toThrowError();
    });
});

function createPendingResponse(
    start?: {
        status: ResponseStatusCode;
        headers?: Headers;
    },
    body?: {
        chunks: Uint8Array[];
        trailers?: Headers;
    },
) {
    const id = Id.create();
    const { responseStart, responseFrame, responseError } = createPipes();

    if (start) {
        responseStart.sender.send({
            id,
            ...start,
        });
    }

    if (body) {
        for (const chunk of body.chunks) {
            responseFrame.sender.send({
                type: ResponseFrameEventType.BODY_CHUNK,
                chunk,
            });
        }

        responseFrame.sender.send({
            type: ResponseFrameEventType.END,
            trailers: body.trailers,
        });
    }

    return {
        id,
        responseStartSender: responseStart.sender,
        responseFrameSender: responseFrame.sender,
        responseErrorSender: responseError.sender,
        response: new PendingResponse(
            id,
            responseStart.receiver,
            responseFrame.receiver,
            responseError.receiver,
        ),
    };
}

function createPipes() {
    return {
        responseStart: new Oneshot<ResponseStart>(),
        responseFrame: new Mpsc<ResponseFrameEvent>(),
        responseError: new Oneshot<ResponseError>(),
    };
}
