import { describe, expect, it } from "vitest";
import {
    BackgroundProcessor,
    BackgroundProcessorCompoundError,
} from "./background-processor";

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

    it("should throw error in awaitAll if there are errors in any of the tasks", async () => {
        const processor = new BackgroundProcessor();
        const id = processor.queue(Promise.reject(new Error("test")));
        processor.queue(sleep(10));

        await sleep(20);

        expect(processor.hasError(id)).toBeTruthy();
        expect(processor.getError(id)).toBeInstanceOf(Error);
        await expect(processor.awaitAll()).rejects.toThrowError(
            BackgroundProcessorCompoundError,
        );
    });
});

function sleep(ms: number) {
    return new Promise((resolve) => {
        setTimeout(resolve, ms);
    });
}
