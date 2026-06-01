import type { Id } from "@/id";
import type { PendingRequest } from "./client/request";

export abstract class ClientHandle {
    abstract requestWithId(id: Id, path: string): Promise<PendingRequest>;
    abstract request(path: string): Promise<PendingRequest>;

    public static DUMMY = ClientHandle.dummy();
    public static dummy(): ClientHandle {
        return new (class extends ClientHandle {
            requestWithId(_id: Id, _path: string): Promise<PendingRequest> {
                throw new Error(
                    "Dummy client handle does not support requests",
                );
            }
            request(_path: string): Promise<PendingRequest> {
                throw new Error(
                    "Dummy client handle does not support requests",
                );
            }
        })();
    }
}
