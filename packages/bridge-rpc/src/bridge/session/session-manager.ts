import type { Id } from "../../";
import { AsyncMutex } from "../../async-mutex";
import { RequestSession } from "./request-session";
import { ResponseSession } from "./response-session";

export class SessionManagerError extends Error {
    constructor(
        public kind: "SessionIdInUse",
        public id: Id,
    ) {
        super(`Session ID in use: ${id}`);
        this.name = "SessionManagerError";
    }
}

export class SessionManager<TRequestContext, TResponseContext> {
    private requestSessions = new Map<
        bigint,
        AsyncMutex<RequestSession<TRequestContext>>
    >();
    private responseSessions = new Map<
        bigint,
        AsyncMutex<ResponseSession<TResponseContext>>
    >();

    private checkIdInUse(id: Id): void {
        if (
            this.requestSessions.has(id.getValue()) ||
            this.responseSessions.has(id.getValue())
        ) {
            throw new SessionManagerError("SessionIdInUse", id);
        }
    }

    public startRequestSession(id: Id, context: TRequestContext) {
        this.checkIdInUse(id);

        const session = new RequestSession(id, context);
        const entry = new AsyncMutex(session);
        this.requestSessions.set(id.getValue(), entry);

        return entry;
    }

    public getRequestSession(id: Id) {
        return this.requestSessions.get(id.getValue());
    }

    public async closeRequestSession(id: Id): Promise<void> {
        const entry = this.requestSessions.get(id.getValue());
        if (entry) {
            this.requestSessions.delete(id.getValue());
            // Lock the session to ensure no other transitions are happening during close
            await entry.runExclusive(async (session) => {
                await session.close();
            });
        }
    }

    public startResponseSession(id: Id, context: TResponseContext) {
        this.checkIdInUse(id);

        const session = new ResponseSession(id, context);
        const entry = new AsyncMutex(session);
        this.responseSessions.set(id.getValue(), entry);

        return entry;
    }

    public hasResponseSession(id: Id): boolean {
        return this.responseSessions.has(id.getValue());
    }

    public getResponseSession(id: Id) {
        return this.responseSessions.get(id.getValue());
    }

    public async closeResponseSession(id: Id): Promise<void> {
        const entry = this.responseSessions.get(id.getValue());
        if (entry) {
            this.responseSessions.delete(id.getValue());
            await entry.runExclusive(async (session) => {
                await session.close();
            });
        }
    }
}
