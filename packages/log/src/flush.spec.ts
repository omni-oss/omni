import { afterEach, describe, expect, it, vi } from "vitest";
import { flushLogs, type LogDrain, registerLogDrain } from "./flush";

describe("flushLogs / registerLogDrain", () => {
    // The drain registry is process-global; unregister everything a test
    // registers so tests stay independent.
    let disposers: Array<() => void> = [];

    function track(drain: LogDrain): () => void {
        const dispose = registerLogDrain(drain);
        disposers.push(dispose);
        return dispose;
    }

    afterEach(() => {
        for (const dispose of disposers) dispose();
        disposers = [];
    });

    it("resolves immediately when nothing is registered", async () => {
        await expect(flushLogs()).resolves.toBeUndefined();
    });

    it("awaits every registered drain before resolving", async () => {
        let aDone = false;
        let bDone = false;

        track(async () => {
            await delay(10);
            aDone = true;
        });
        track(async () => {
            await delay(20);
            bDone = true;
        });

        await flushLogs();

        expect(aDone).toBe(true);
        expect(bDone).toBe(true);
    });

    it("invokes each drain once per flush (repeatable)", async () => {
        const drain = vi.fn<LogDrain>(async () => {});
        track(drain);

        await flushLogs();
        await flushLogs();

        expect(drain).toHaveBeenCalledTimes(2);
    });

    it("swallows drain rejections and still awaits the others", async () => {
        let goodDone = false;

        track(async () => {
            throw new Error("boom");
        });
        track(async () => {
            await delay(10);
            goodDone = true;
        });

        await expect(flushLogs()).resolves.toBeUndefined();
        expect(goodDone).toBe(true);
    });

    it("stops awaiting a drain once its disposer runs", async () => {
        const drain = vi.fn<LogDrain>(async () => {});
        const dispose = track(drain);

        dispose();
        await flushLogs();

        expect(drain).not.toHaveBeenCalled();
    });
});

function delay(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
