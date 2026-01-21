import type { Id } from "../../";
import { RequestStateMachine } from "./request-state-machine";

export class RequestSession<TContext> {
    private readonly _state: RequestStateMachine = new RequestStateMachine();

    constructor(
        public readonly id: Id,
        public readonly context: TContext,
    ) {}

    get state() {
        return this._state;
    }

    async close() {
        // do nothing for now
    }
}
