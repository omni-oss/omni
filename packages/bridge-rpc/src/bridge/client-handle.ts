import type { Id } from "@/id";
import type { PendingRequest } from "./client/request";

export interface ClientHandle {
    requestWithId(id: Id, path: string): Promise<PendingRequest>;
}
