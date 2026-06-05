import { ClientHandle } from "./client-handle";
import type { Request } from "./server/request";
import type { PendingResponse } from "./server/response";

export class ServiceContext {
    constructor(
        public readonly request: Request,
        public readonly response: PendingResponse,
        public readonly client: ClientHandle,
    ) {}

    public static fromRequestAndResponse(
        request: Request,
        response: PendingResponse,
    ) {
        return new ServiceContext(request, response, ClientHandle.DUMMY);
    }
}

export interface Service {
    run(context: ServiceContext): Promise<void>;
}
