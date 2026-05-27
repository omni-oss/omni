import type { Id } from "../../";
import { ResponseStateMachine } from "./response-state-machine";

export class ResponseSession<TContext> {
    private readonly _state: ResponseStateMachine = new ResponseStateMachine();

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
