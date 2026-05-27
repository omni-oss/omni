import { describe, expect, it } from "vitest";
import { Id } from "../../";
import type { Headers, Trailers } from "../dyn-map";
import { RequestErrorCode } from "../error-code";
import {
    type RequestEvent,
    RequestEventType,
    RequestStateMachine,
} from "./request-state-machine"; // Adjust path accordingly

describe("RequestStateMachine", () => {
    // Equivalent to fn new() -> (Id, RequestStateMachine)
    const createTestSetup = () => {
        return { id: Id.create(), rsm: new RequestStateMachine() };
    };

    // Helper functions to match Rust test setup
    const startEvent = (
        id: Id,
        path: string,
        headers?: Headers,
    ): RequestEvent => ({
        type: RequestEventType.START,
        data: { id, path, headers },
    });

    const bodyChunkEvent = (id: Id, chunk: number[]): RequestEvent => ({
        type: RequestEventType.BODY_CHUNK,
        data: { id, chunk: new Uint8Array(chunk) },
    });

    const endEvent = (id: Id, trailers?: Trailers): RequestEvent => ({
        type: RequestEventType.END,
        data: { id, trailers },
    });

    it("should support normal transitions", () => {
        const { id, rsm } = createTestSetup();

        // Start Transition
        expect(rsm.transition(startEvent(id, "/"))).toEqual({
            type: "Start",
            id: id,
            path: "/",
            headers: undefined,
        });

        // Body Chunk 1
        expect(rsm.transition(bodyChunkEvent(id, [1, 2, 3]))).toEqual({
            type: "BodyChunk",
            chunk: new Uint8Array([1, 2, 3]),
        });

        // Body Chunk 2
        expect(rsm.transition(bodyChunkEvent(id, [4, 5, 6]))).toEqual({
            type: "BodyChunk",
            chunk: new Uint8Array([4, 5, 6]),
        });

        // End Transition
        const finalEvent = endEvent(id);
        expect(rsm.transition(finalEvent)).toEqual({
            type: "End",
            trailers: undefined,
        });

        // Should not transition after End
        expect(() => rsm.transition(finalEvent)).toThrowError(
            expect.objectContaining({ kind: "Ended" }),
        );
    });

    it("should support ending without body", () => {
        const { id, rsm } = createTestSetup();

        expect(rsm.transition(startEvent(id, "/"))).toEqual(
            expect.objectContaining({ type: "Start" }),
        );

        expect(rsm.transition(endEvent(id))).toEqual(
            expect.objectContaining({ type: "End" }),
        );

        // Transition after End should fail
        expect(() =>
            rsm.transition(bodyChunkEvent(id, [1, 2, 3])),
        ).toThrowError(expect.objectContaining({ kind: "Ended" }));
    });

    it("should not allow transitions after errored", () => {
        const { id, rsm } = createTestSetup();

        rsm.transition(startEvent(id, "/"));
        rsm.transition(bodyChunkEvent(id, [1, 2, 3]));

        const errorData = {
            id,
            code: RequestErrorCode.TIMED_OUT,
            message: "error",
        };
        const errorEvent: RequestEvent = {
            type: RequestEventType.ERROR,
            data: errorData,
        };

        expect(rsm.transition(errorEvent)).toEqual({
            type: "Error",
            error: errorData,
        });

        // Should not transition after Errored state
        const endAfterError = endEvent(id);
        expect(() => rsm.transition(endAfterError)).toThrowError(
            expect.objectContaining({ kind: "Errored" }),
        );
    });

    it("should not allow transitions with mismatching IDs", () => {
        const { id, rsm } = createTestSetup();
        const incorrectId = Id.create();

        // Initialize with first ID
        rsm.transition(startEvent(id, "/"));

        // Transition with mismatching ID should throw InvalidId
        expect(() =>
            rsm.transition(bodyChunkEvent(incorrectId, [1, 2, 3])),
        ).toThrowError(expect.objectContaining({ kind: "InvalidId" }));

        // Machine should still function correctly for the original ID
        expect(rsm.transition(bodyChunkEvent(id, [1, 2, 3]))).toEqual({
            type: "BodyChunk",
            chunk: new Uint8Array([1, 2, 3]),
        });
    });
});
