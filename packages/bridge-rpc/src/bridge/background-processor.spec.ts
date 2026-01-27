import { describe, expect, it } from "vitest";
import { BackgroundProcessor } from "./background-processor";

describe("BackgroundProcesser", () => {
    it("should be able to run a task", async () => {
        const processor = new BackgroundProcessor();
        const output: number[] = [];
        processor.queue(sleep(10).then(() => output.push(1)));
        processor.queue(sleep(10).then(() => output.push(2)));

        await sleep(20);

        expect(output).toEqual([1, 2]);
        await processor.awaitAll();
    });

    it("should be able to handle errors", async () => {
        const processor = new BackgroundProcessor();
        const id = processor.queue(Promise.reject(new Error("test")));

        await sleep(20);

        expect(processor.hasError(id)).toBeTruthy();
        expect(processor.getError(id)).toBeInstanceOf(Error);
        await processor.awaitAll();
    });
});

function sleep(ms: number) {
    return new Promise((resolve) => {
        setTimeout(resolve, ms);
    });
}
