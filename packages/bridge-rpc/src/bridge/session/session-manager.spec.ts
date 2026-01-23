import { describe, expect, it } from "vitest";
import { Id } from "../..";
import { SessionManager, SessionManagerError } from "./session-manager";

describe("SessionManager", () => {
    const generateId = () => Id.create();

    it("should start and close a session", async () => {
        const sm = new SessionManager<void, void>();
        const id1 = generateId();
        const id2 = generateId();

        const req1 = sm.startRequestSession(id1, undefined);
        const req2 = sm.startRequestSession(id2, undefined);

        expect(await req1.runExclusive((session) => session.id)).toBe(id1);
        expect(await req2.runExclusive((session) => session.id)).toBe(id2);

        await sm.closeRequestSession(id1);
        await sm.closeRequestSession(id2);

        expect(sm.getRequestSession(id1)).toBeUndefined();
    });

    it("should throw when trying to start a session with an ID that is already in use", async () => {
        const sm = new SessionManager<void, void>();
        const id = generateId();

        sm.startRequestSession(id, undefined);

        // Attempting to start with same ID should throw
        expect(() => sm.startRequestSession(id, undefined)).toThrow(
            SessionManagerError,
        );

        try {
            sm.startRequestSession(id, undefined);
        } catch (e) {
            const err = e as SessionManagerError;
            expect(err.kind).toBe("SessionIdInUse");
            expect(err.id).toBe(id);
        }
    });

    it("should respect concurrency limits via mutex", async () => {
        const sm = new SessionManager<void, void>();
        const id = generateId();
        const entry = sm.startRequestSession(id, undefined);

        let counter = 0;

        // Simulate two async operations trying to modify the session
        const op = async () => {
            await entry.runExclusive(async () => {
                const local = counter;
                await new Promise((res) => setTimeout(res, 10)); // simulated delay
                counter = local + 1;
            });
        };

        await Promise.all([op(), op()]);

        // If mutex works, counter should be 2. If not (race condition), it would be 1.
        expect(counter).toBe(2);
    });
});
