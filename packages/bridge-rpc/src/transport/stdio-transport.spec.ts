import { describe, expect, it } from "vitest";

describe("StdioTransport", () => {
    function createStdio() {
        let controller!: ReadableStreamDefaultController<Uint8Array>;
        const input = new ReadableStream<Uint8Array>({
            start(ctrl) {
                controller = ctrl;
            },
        });

        const writtenChunks: Uint8Array[] = [];
        const output = new WritableStream<Uint8Array>({
            write(chunk) {
                writtenChunks.push(chunk);
            },
        });

        return { input, output, controller, writtenChunks };
    }

    it("test1", async () => {
        const stdio = createStdio();

        expect(stdio).toBeDefined();
        expect(true).toBe(true);
    });
});
