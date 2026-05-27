import { describe, expect, it, vi } from "vitest";
import { Id } from "@/id";
import { encode } from "./codec-utils";
import { Frame } from "./frame";
import { FrameTransporter } from "./frame-transporter";

describe("FrameTransporter", () => {
    it("should start and stop", async () => {
        const senderFn = vi.fn();
        const transporter = new FrameTransporter(senderFn);

        await expect(transporter.start()).resolves.toBeUndefined();
        expect(transporter.isRunning).toBeTruthy();

        await expect(transporter.stop()).resolves.toBeUndefined();
        expect(transporter.isRunning).toBeFalsy();

        expect(senderFn).toHaveBeenCalledTimes(0);
    });

    it("should send frame", async () => {
        const senderFn = vi.fn();
        const transporter = new FrameTransporter(senderFn);

        await expect(transporter.start()).resolves.toBeUndefined();
        expect(transporter.isRunning).toBeTruthy();

        const frame = Frame.requestStart(Id.create(), "test");
        transporter.sender.send(frame);

        await expect(transporter.stop()).resolves.toBeUndefined();
        expect(transporter.isRunning).toBeFalsy();

        expect(senderFn).toHaveBeenCalledWith(encode(frame));
    });
});
