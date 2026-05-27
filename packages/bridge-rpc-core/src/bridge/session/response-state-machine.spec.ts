import { describe, expect, it } from "vitest";
import { Id } from "../..";
import type { Headers, Trailers } from "../dyn-map";
import { ResponseErrorCode } from "../error-code";
import { ResponseStatusCode } from "../status-code";
import {
    type ResponseEvent,
    ResponseEventType,
    ResponseStateMachine,
} from "./response-state-machine"; // Adjust path accordingly

describe("ResponseStateMachine", () => {
    // Equivalent to fn new() -> (Id, ResponseStateMachine)
    const createTestSetup = () => {
        return { id: Id.create(), rsm: new ResponseStateMachine() };
    };

    // Equivalent to fn start_event
    const startEvent = (
        id: Id,
        status: ResponseStatusCode,
        headers?: Headers,
    ): ResponseEvent => ({
        type: ResponseEventType.START,
        data: { id, status, headers },
    });

    // Equivalent to fn body_chunk_event
    const bodyChunkEvent = (id: Id, chunk: number[]): ResponseEvent => ({
        type: ResponseEventType.BODY_CHUNK,
        data: { id, chunk: new Uint8Array(chunk) },
    });

    // Equivalent to fn end_event
    const endEvent = (id: Id, trailers?: Trailers): ResponseEvent => ({
        type: ResponseEventType.END,
        data: { id, trailers },
    });

    it("should follow normal path successfully", () => {
        const { id, rsm } = createTestSetup();

        // Start Transition
        expect(
            rsm.transition(startEvent(id, ResponseStatusCode.SUCCESS)),
        ).toEqual({
            type: "Start",
            id: id,
            status: ResponseStatusCode.SUCCESS,
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

        // End
        const finalEvent = endEvent(id);
        expect(rsm.transition(finalEvent)).toEqual({
            type: "End",
            trailers: undefined,
        });

        // Subsequent transition should throw "Ended"
        expect(() => rsm.transition(finalEvent)).toThrowError(
            expect.objectContaining({ kind: "Ended" }),
        );
    });

    it("should handle transition with no body", () => {
        const { id, rsm } = createTestSetup();

        expect(
            rsm.transition(startEvent(id, ResponseStatusCode.SUCCESS)),
        ).toEqual(expect.objectContaining({ type: "Start" }));

        expect(rsm.transition(endEvent(id))).toEqual(
            expect.objectContaining({ type: "End" }),
        );

        // Fail to add body after end
        expect(() =>
            rsm.transition(bodyChunkEvent(id, [1, 2, 3])),
        ).toThrowError(expect.objectContaining({ kind: "Ended" }));
    });

    it("should support error event", () => {
        const { id, rsm } = createTestSetup();

        rsm.transition(startEvent(id, ResponseStatusCode.SUCCESS));
        rsm.transition(bodyChunkEvent(id, [1, 2, 3]));

        const error = {
            id,
            code: ResponseErrorCode.UNEXPECTED_FRAME,
            message: "error",
        };
        const errorEvent: ResponseEvent = {
            type: ResponseEventType.ERROR,
            data: error,
        };

        expect(rsm.transition(errorEvent)).toEqual({
            type: "Error",
            error: error,
        });

        // Cannot transition after Error state
        expect(() => rsm.transition(endEvent(id))).toThrowError(
            expect.objectContaining({ kind: "Errored" }),
        );
    });

    it("should not transition with different IDs", () => {
        const { id, rsm } = createTestSetup();
        const incorrectId = Id.create();

        // Lock the machine to the first ID
        rsm.transition(startEvent(id, ResponseStatusCode.SUCCESS));

        // Attempting a transition with a different ID
        expect(() =>
            rsm.transition(bodyChunkEvent(incorrectId, [1, 2, 3])),
        ).toThrowError(expect.objectContaining({ kind: "InvalidId" }));

        // Machine should NOT be corrupted and still accept the original ID
        expect(rsm.transition(bodyChunkEvent(id, [1, 2, 3]))).toEqual({
            type: "BodyChunk",
            chunk: new Uint8Array([1, 2, 3]),
        });
    });
});
