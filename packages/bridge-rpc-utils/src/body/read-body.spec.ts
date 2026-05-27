import { Id } from "@omni-oss/bridge-rpc-core";
import type { RequestError } from "@omni-oss/bridge-rpc-core/frame";
import { Request, RequestFrameEvent } from "@omni-oss/bridge-rpc-core/server";
import { Oneshot } from "@omni-oss/channels";
import { describe, expect, test } from "vitest";
import { combine, readBody, readBodyAsJson, readBodyAsText } from "./read-body";

describe("readBody", () => {
    test("should merge chunk arrays", async () => {
        const body = await readBody(
            createRequest([
                new Uint8Array([1, 2, 3]),
                new Uint8Array([4, 5, 6]),
                new Uint8Array([7, 8, 9]),
            ]),
        );

        expect(body).toEqual(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9]));
    });
});

describe("readBodyAsText", () => {
    test("should read body as text", async () => {
        const encoder = new TextEncoder();
        const request = createRequest([encoder.encode("Hello, World!")]);

        const text = await readBodyAsText(request);
        expect(text).toBe("Hello, World!");
    });
});

describe("readBodyAsJson", () => {
    test("should read body as JSON", async () => {
        const encoder = new TextEncoder();
        const request = createRequest([
            encoder.encode(JSON.stringify({ message: "Hello, World!" })),
        ]);

        const json = await readBodyAsJson(request);
        expect(json).toEqual({ message: "Hello, World!" });
    });
});

describe("combine", () => {
    test("should combine multiple Uint8Arrays into one", () => {
        const arrays = [
            new Uint8Array([1, 2, 3]),
            new Uint8Array([4, 5]),
            new Uint8Array([6, 7, 8, 9]),
        ];

        const combined = combine(arrays);

        expect(combined).toEqual(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9]));
    });
});

function createRequest(body: Uint8Array[]) {
    const requestError = new Oneshot<RequestError>();
    return new Request(
        Id.create(),
        "test_path",
        {},
        (async function* () {
            for (const chunk of body) {
                await new Promise((resolve) => setTimeout(resolve, 1)); // Simulate async chunk arrival
                yield RequestFrameEvent.bodyChunk(chunk);
            }

            yield RequestFrameEvent.end();
            return;
        })(),
        requestError.receiver,
    );
}
