import type { ClientHandle } from "./client-handle";
import type { Request } from "./server/request";
import type { PendingResponse } from "./server/response";

export class ServiceContext {
    constructor(
        public readonly request: Request,
        public readonly response: PendingResponse,
        public readonly client: ClientHandle,
    ) {}
}

export type Service = {
    run: (context: ServiceContext) => Promise<void>;
};
