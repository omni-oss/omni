import { describe, expect, it } from "vitest";
import { Oneshot, OneshotClosedError, OneshotValueSentError } from "./oneshot";

describe("Oneshot", () => {
    it("should not have value by default", async () => {
        const oneshot = new Oneshot<void>();

        expect(oneshot.receiver.hasValue()).toBeFalsy();
    });

    it("should not be closed by default", async () => {
        const oneshot = new Oneshot<void>();

        expect(oneshot.receiver.isClosed()).toBeFalsy();
    });

    it("should not be sent by default", async () => {
        const oneshot = new Oneshot<void>();

        expect(oneshot.sender.isSent()).toBeFalsy();
    });

    it("should have value after sender sends value", async () => {
        const oneshot = new Oneshot<number>();

        oneshot.sender.send(1);

        expect(oneshot.receiver.hasValue()).toBeTruthy();
        await expect(oneshot.receiver.receive()).resolves.toBe(1);
    });

    it("should error if value is sent twice", async () => {
        const oneshot = new Oneshot<number>();

        oneshot.sender.send(1);

        expect(oneshot.receiver.hasValue()).toBeTruthy();
        await expect(oneshot.receiver.receive()).resolves.toBe(1);
        expect(() => oneshot.sender.send(1)).toThrowError(
            OneshotValueSentError,
        );
    });

    it("should error on send if it is closed", async () => {
        const oneshot = new Oneshot<number>();

        oneshot.receiver.close();

        expect(() => oneshot.sender.send(1)).toThrowError(OneshotClosedError);
        await expect(oneshot.receiver.receive()).rejects.toThrowError(
            OneshotClosedError,
        );
    });

    it("should error on receive if it is closed", async () => {
        const oneshot = new Oneshot<number>();

        oneshot.receiver.close();

        await expect(oneshot.receiver.receive()).rejects.toThrowError(
            OneshotClosedError,
        );
    });

    it("should error on close if it is closed before sending a value", async () => {
        const oneshot = new Oneshot<number>();

        oneshot.receiver.close();

        expect(() => oneshot.receiver.close()).toThrowError(OneshotClosedError);
        await expect(oneshot.receiver.receive()).rejects.toThrowError(
            OneshotClosedError,
        );
    });
});
