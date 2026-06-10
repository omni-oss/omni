import type {
    ResponseStatusCode,
    SerializableValue,
} from "@omni-oss/bridge-rpc-core";
import { readBody } from "@omni-oss/bridge-rpc-utils";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

export interface CallResult<T = unknown> {
    status: ResponseStatusCode;
    returns: T;
    body: Uint8Array;
}

/**
 * Make a request to a `/fs/*` service where all data is in headers.
 * No body is sent; the response may carry data in either headers (`returns`)
 * or a body (for read operations).
 */
export async function call<T = unknown>(
    servicePath: string,
    params?: Record<string, unknown>,
): Promise<CallResult<T>> {
    const req = await RsRpcClient.request(servicePath);
    const active = await req.start({
        parameters: params as SerializableValue,
    });
    const response = await active.end().then((r) => r.wait());
    const body = await readBody(response);
    return {
        status: response.status,
        returns: (response.headers as { returns?: T } | undefined)
            ?.returns as T,
        body,
    };
}

/**
 * Make a request to a `/fs/*` service where input data is sent in the body
 * (e.g. write operations).
 */
export async function callWithBody<T = unknown>(
    servicePath: string,
    params: Record<string, unknown>,
    bodyData: Uint8Array,
): Promise<CallResult<T>> {
    const req = await RsRpcClient.request(servicePath);
    const active = await req.start({
        parameters: params as SerializableValue,
    });
    await active.writeBodyChunk(bodyData);
    const response = await active.end().then((r) => r.wait());
    const body = await readBody(response);
    return {
        status: response.status,
        returns: (response.headers as { returns?: T } | undefined)
            ?.returns as T,
        body,
    };
}
