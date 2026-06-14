import type { Id } from "../../";
import { RequestStateMachine } from "./request-state-machine";
import type { ClosableSessionContext } from "./session-context";

export class RequestSession<TContext extends ClosableSessionContext> {
    private readonly _state: RequestStateMachine = new RequestStateMachine();

    constructor(
        public readonly id: Id,
        public readonly context: TContext,
    ) {}

    get state() {
        return this._state;
    }

    async close() {
        await this.context?.close?.();
    }
}
