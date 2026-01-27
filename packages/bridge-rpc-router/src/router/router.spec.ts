import { Id, Mpsc, Oneshot, ResponseStatusCode } from "@omni-oss/bridge-rpc";
import {
    type Frame,
    FrameType,
    type RequestError,
} from "@omni-oss/bridge-rpc/frame";
import {
    PendingResponse,
    Request,
    type RequestFrameEvent,
    RequestFrameEventType,
} from "@omni-oss/bridge-rpc/server";
import { describe, expect, it, vi } from "vitest";
import { Router } from "./router";

describe("Router", () => {
    it("should be able to add service", () => {
        const router = new Router();
        const service = {
            run: () => Promise.resolve(),
        };

        router.addService("/test", service);

        expect(router.hasService("/test")).toBeTruthy();
    });

    it("should be able to add handler", () => {
        const router = new Router();
        const handler = () => Promise.resolve();

        router.addHandler("/test", handler);

        expect(router.hasService("/test")).toBeTruthy();
    });

    it("should be able to run service", async () => {
        const router = new Router();
        const service = {
            run: vi.fn().mockImplementation(() => Promise.resolve()),
        };

        router.addService("/test", service);

        const testContext = createTestContext("/test");

        await router.run(testContext.context);

        expect(service.run).toHaveBeenCalledTimes(1);
        expect(service.run).toHaveBeenCalledWith(testContext.context);
    });

    it("should be able to run handler", async () => {
        const router = new Router();
        const handler = vi.fn().mockImplementation(async () => {
            return Promise.resolve();
        });

        router.addHandler("/test", handler);

        const testContext = createTestContext("/test");

        await router.run(testContext.context);

        expect(handler).toHaveBeenCalledTimes(1);
        expect(handler).toHaveBeenCalledWith(testContext.context);
    });

    it("should respond with NO_HANDLER_FOR_PATH if no service is found", async () => {
        const router = new Router();

        const testContext = createTestContext("/test");

        await router.run(testContext.context);

        const response = await testContext.responseFrameReceiver.receive();

        expect(response).toBeDefined();
        if (!response) return;
        expect(response.type).toEqual(FrameType.RESPONSE_START);
        if (response.type !== FrameType.RESPONSE_START) return;
        expect(response.data.status).toEqual(
            ResponseStatusCode.NO_HANDLER_FOR_PATH,
        );
    });
});

function createTestContext<T = unknown>(
    path: string,
    headers?: Record<string, string>,
    body?: T,
) {
    const id = Id.create();
    const requestError = new Oneshot<RequestError>();
    const frameMpsc = new Mpsc<Frame>();

    return {
        context: {
            request: new Request(
                id,
                path,
                headers,
                createTestBody(body),
                requestError.receiver,
            ),
            response: new PendingResponse(id, frameMpsc.sender),
        },
        responseFrameReceiver: frameMpsc.receiver,
        requestErrorSender: requestError.sender,
    };
}

async function* createTestBody(
    body: unknown,
): AsyncIterable<RequestFrameEvent> {
    if (body !== undefined) {
        await sleep(1);
        const bytes = new Uint8Array(
            new TextEncoder().encode(JSON.stringify(body)),
        );
        yield {
            chunk: bytes,
            type: RequestFrameEventType.BODY_CHUNK,
        };
    }
    await sleep(1);
    yield {
        type: RequestFrameEventType.END,
    };
}

async function sleep(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
