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
});
