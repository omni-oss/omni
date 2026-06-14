import type { Id } from "../../";
import { ResponseStateMachine } from "./response-state-machine";
import type { ClosableSessionContext } from "./session-context";

export class ResponseSession<TContext extends ClosableSessionContext> {
    private readonly _state: ResponseStateMachine = new ResponseStateMachine();

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
