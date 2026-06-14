import { describe, expect, it } from "vitest";
import { Mpsc } from "./mpsc"; // Assume the code from previous response is here

describe("Mpsc", () => {
    it("should receive values in the order they were sent", async () => {
        const { sender: tx, receiver: rx } = new Mpsc<number>();

        tx.send(1);
        tx.send(2);

        const val1 = await rx.next();
        const val2 = await rx.next();

        expect(val1).toEqual({ done: false, value: 1 });
        expect(val2).toEqual({ done: false, value: 2 });
    });

    it("should resolve a pending receive promise when a value is sent", async () => {
        const { sender: tx, receiver: rx } = new Mpsc<string>();

        const receivePromise = rx.next();

        // Simulate some async delay
        setTimeout(() => tx.send("hello"), 10);

        const result = await receivePromise;
        expect(result).toEqual({ done: false, value: "hello" });
    });

    it("should handle multiple producers (clones)", async () => {
        const { sender: tx, receiver: rx } = new Mpsc<string>();
        const tx2 = tx.clone();

        tx.send("from original");
        tx2.send("from clone");

        expect(await rx.next()).toEqual({
            done: false,
            value: "from original",
        });
        expect(await rx.next()).toEqual({ done: false, value: "from clone" });
    });

    it("should work with for-await-of loops", async () => {
        const { sender: tx, receiver: rx } = new Mpsc<number>();
        const results: number[] = [];

        // Producer
        tx.send(10);
        tx.send(20);
        tx.close();

        // Consumer
        for await (const val of rx) {
            results.push(val);
        }

        expect(results).toEqual([10, 20]);
    });

    it("should return done: true when receiver is closed", async () => {
        const { sender: tx, receiver: rx } = new Mpsc<number>();

        rx.close();

        const result = await rx.next();
        expect(result.done).toBe(true);
        expect(() => tx.send(1)).toThrow();
    });

    it("should return done: true when sender is closed", async () => {
        const { sender: tx, receiver: rx } = new Mpsc<number>();

        tx.close();

        const result = await rx.next();
        expect(result.done).toBe(true);
    });

    it("should allow draining the buffer after the sender is closed", async () => {
        const { sender: tx, receiver: rx } = new Mpsc<number>();

        tx.send(1);
        tx.send(2);
        tx.close();

        expect(await rx.next()).toEqual({ done: false, value: 1 });
        expect(await rx.next()).toEqual({ done: false, value: 2 });
        expect(await rx.next()).toEqual({ done: true, value: undefined });
    });

    it("should wake up all waiting receivers with done: true on close", async () => {
        const { receiver: rx } = new Mpsc<number>();

        const p1 = rx.next();
        const p2 = rx.next();

        rx.close();

        const [res1, res2] = await Promise.all([p1, p2]);
        expect(res1.done).toBe(true);
        expect(res2.done).toBe(true);
    });

    describe("bounded (backpressure)", () => {
        it("rejects an invalid capacity", () => {
            expect(() => new Mpsc<number, number>(0)).toThrow();
            expect(() => new Mpsc<number, number>(-1)).toThrow();
            expect(() => new Mpsc<number, number>(1.5)).toThrow();
        });

        it("resolves sends immediately while there is spare capacity", async () => {
            const { sender: tx } = new Mpsc<number, number>(2);

            let resolved = 0;
            await tx.send(1).then(() => resolved++);
            await tx.send(2).then(() => resolved++);

            expect(resolved).toBe(2);
        });

        it("applies backpressure once the buffer is full and resolves when a slot frees", async () => {
            const { sender: tx, receiver: rx } = new Mpsc<number, number>(1);

            // First send fits in the buffer and resolves immediately.
            await tx.send(1);

            // Second send must wait for a free slot.
            let thirdResolved = false;
            const pending = tx.send(2).then(() => {
                thirdResolved = true;
            });

            // Give the microtask queue a chance — it must still be pending.
            await Promise.resolve();
            expect(thirdResolved).toBe(false);

            // Draining one item frees a slot and unblocks the pending send.
            expect(await rx.next()).toEqual({ done: false, value: 1 });
            await pending;
            expect(thirdResolved).toBe(true);

            // The previously backpressured value is now buffered.
            expect(await rx.next()).toEqual({ done: false, value: 2 });
        });

        it("preserves FIFO order across backpressured sends", async () => {
            const { sender: tx, receiver: rx } = new Mpsc<number, number>(1);

            await tx.send(1);
            // These two are backpressured (not awaited yet).
            const p2 = tx.send(2);
            const p3 = tx.send(3);

            const received: number[] = [];
            received.push((await rx.next()).value as number);
            received.push((await rx.next()).value as number);
            received.push((await rx.next()).value as number);
            await Promise.all([p2, p3]);

            expect(received).toEqual([1, 2, 3]);
        });

        it("hands off directly to a waiting receiver without consuming capacity", async () => {
            const { sender: tx, receiver: rx } = new Mpsc<number, number>(1);

            const pending = rx.next();
            // A receiver is already waiting, so this resolves immediately even
            // though we then fill the buffer.
            await tx.send(1);
            expect(await pending).toEqual({ done: false, value: 1 });

            // Buffer is empty again, so another send fits without backpressure.
            await tx.send(2);
        });

        it("rejects pending backpressured sends when closed", async () => {
            const { sender: tx } = new Mpsc<number, number>(1);

            await tx.send(1);
            const pending = tx.send(2);

            tx.close();

            await expect(pending).rejects.toThrow();
        });

        it("trySend returns false when the buffer is full", async () => {
            const { sender: tx } = new Mpsc<number, number>(1);

            expect(tx.trySend(1)).toBe(true);
            expect(tx.trySend(2)).toBe(false);
        });
    });
});
