import { Mpsc, Oneshot } from "@omni-oss/channels";
import { describe, expect, it } from "vitest";
import { Id } from "@/id";
import { readAll } from "../byte-array-utils";
import type { Headers } from "../dyn-map";
import { RequestErrorCode } from "../error-code";
import type { RequestError } from "../frame";
import {
    Request,
    type RequestFrameEvent,
    RequestFrameEventType,
} from "./request";

describe("Response", () => {
    it("should be able to read body chunk", async () => {
        const data = new Uint8Array([1, 2, 3]);
        const { request } = createRequest(
            "/test",
            {},
            {
                chunks: [data],
                trailers: {},
            },
        );

        const bytes = await readAll(request.readBody());

        expect(bytes).toBeDefined();
        expect(bytes).toEqual(data);
    });

    it("should be able to receive trailers", async () => {
        const trailers = { test: 1232 };
        const { request } = createRequest(
            "/test",
            {},
            {
                chunks: [new Uint8Array([1, 2, 3])],
                trailers,
            },
        );

        await readAll(request.readBody());

        expect(request.trailers).toBeDefined();
        expect(request.trailers).toEqual(trailers);
    });

    it("should throw error when reading body chunk after end", async () => {
        const { request } = createRequest(
            "/test",
            {},
            {
                chunks: [new Uint8Array([1, 2, 3])],
                trailers: {},
            },
        );

        await readAll(request.readBody());

        await expect(readAll(request.readBody())).rejects.toThrowError();
    });

    it("should throw error when received error frame", async () => {
        const { id, request, requestFrameError } = createRequest(
            "/test",
            {},
            {
                chunks: [new Uint8Array([1, 2, 3])],
                trailers: {},
            },
        );

        requestFrameError.send({
            code: RequestErrorCode.TIMED_OUT,
            id,
            message: "test",
        });

        await expect(readAll(request.readBody())).rejects.toThrowError();
    });
});

function createRequest(
    path: string,
    headers: Headers | undefined,
    body?: {
        chunks: Uint8Array[];
        trailers?: Headers;
    },
) {
    const id = Id.create();
    const { requestFrame, requestError } = createPipes();

    if (body) {
        for (const chunk of body.chunks) {
            requestFrame.sender.send({
                type: RequestFrameEventType.BODY_CHUNK,
                chunk,
            });
        }

        requestFrame.sender.send({
            type: RequestFrameEventType.END,
            trailers: body.trailers,
        });
    }

    return {
        id,
        requestFrameSender: requestFrame.sender,
        requestFrameError: requestError.sender,
        request: new Request(
            id,
            path,
            headers,
            requestFrame.receiver,
            requestError.receiver,
        ),
    };
}

function createPipes() {
    return {
        requestFrame: new Mpsc<RequestFrameEvent>(),
        requestError: new Oneshot<RequestError>(),
    };
}
