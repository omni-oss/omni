import {
    type ClientHandle,
    type Headers,
    ResponseStatusCode,
    type SerializableValue,
} from "@omni-oss/bridge-rpc-core";
import type { Response } from "@omni-oss/bridge-rpc-core/client";
import { combine } from "@omni-oss/bridge-rpc-utils/body";
import { PARAMETERS_HEADER, RETURNS_HEADER } from "./options";

/**
 * Internal helpers for invoking the Rust-side `bridge_rpc_services` services
 * from the JS side, using the shared wire conventions:
 *
 * - Trivial request inputs live in the `parameters` request header (see
 *   {@link PARAMETERS_HEADER}). Trivial response outputs live in the
 *   `returns` response header (see {@link RETURNS_HEADER}). Encoding is
 *   handled by the protocol layer so we just pass plain JS objects.
 * - Bulk content (file bytes, file text) lives in the body, split into
 *   chunks of at most `maxChunkSize` bytes.
 */

/** Builds a `Headers` map containing only the `parameters` entry. */
export function buildParametersHeaders(
    parameters?: SerializableValue,
): Headers | undefined {
    if (parameters === undefined) {
        return undefined;
    }
    return { [PARAMETERS_HEADER]: parameters };
}

/**
 * Reads the `returns` header from a response and casts it to `T`. Throws
 * if the header is missing.
 */
export function readResponseReturns<T>(response: Response): T {
    const value = response.headers?.[RETURNS_HEADER];
    if (value === undefined) {
        throw new Error(`Response is missing the \`${RETURNS_HEADER}\` header`);
    }
    return value as T;
}

/** Reads the entire body of a response into a single `Uint8Array`. */
export async function readResponseBody(
    response: Response,
): Promise<Uint8Array> {
    const chunks: Uint8Array[] = [];
    for await (const chunk of response.readBody()) {
        chunks.push(chunk);
    }
    if (chunks.length === 0) {
        return new Uint8Array(0);
    }
    if (chunks.length === 1) {
        return chunks[0] as Uint8Array;
    }
    return combine(chunks);
}

/**
 * Throws a descriptive `Error` for any non-success status. The body is
 * fully consumed even on failure so the underlying connection can move on.
 */
export async function ensureSuccess(
    path: string,
    response: Response,
): Promise<Response> {
    if (!response.status.equals(ResponseStatusCode.SUCCESS)) {
        // Best effort: drain the body so the underlying stream is unblocked.
        try {
            await readResponseBody(response);
        } catch {
            // ignore - the original status code is already informative.
        }
        throw new Error(
            `RPC call to \`${path}\` failed with status ${response.status.toString()}`,
        );
    }
    return response;
}

/**
 * Issues a request that has only `parameters` in headers and an empty
 * body. Always drains the response body so the underlying stream is
 * released even when the caller only cares about the headers.
 *
 * Returns the unmodified `Response`, which still exposes the response
 * `headers` even after the body has been drained.
 */
export async function callWithParameters(
    client: ClientHandle,
    path: string,
    parameters?: SerializableValue,
): Promise<Response> {
    const pending = await client.request(path);
    const active = await pending.start(buildParametersHeaders(parameters));
    const pendingResponse = await active.end();
    const response = await pendingResponse.wait();
    await ensureSuccess(path, response);
    // Drain any (typically empty) body so the framework can release the
    // stream cleanly.
    await readResponseBody(response);
    return response;
}

/**
 * Like {@link callWithParameters} but does NOT drain the response body.
 * Use this when the caller is going to call {@link readResponseBody}
 * itself to consume bulk content (e.g. `readFileAsString`,
 * `readFileAsBytes`).
 */
export async function callExpectingBody(
    client: ClientHandle,
    path: string,
    parameters?: SerializableValue,
): Promise<Response> {
    const pending = await client.request(path);
    const active = await pending.start(buildParametersHeaders(parameters));
    const pendingResponse = await active.end();
    const response = await pendingResponse.wait();
    return ensureSuccess(path, response);
}

/**
 * Issues a request with both `parameters` headers and a body. The body is
 * split into chunks of at most `maxChunkSize` bytes. Drains the response
 * body before returning.
 */
export async function callWithBody(
    client: ClientHandle,
    path: string,
    parameters: SerializableValue | undefined,
    body: Uint8Array,
    maxChunkSize: number,
): Promise<Response> {
    const pending = await client.request(path);
    let active = await pending.start(buildParametersHeaders(parameters));

    if (body.byteLength > 0) {
        for (let offset = 0; offset < body.byteLength; offset += maxChunkSize) {
            const end = Math.min(offset + maxChunkSize, body.byteLength);
            // Slice without copying the underlying buffer.
            const chunk = body.subarray(offset, end);
            active = await active.writeBodyChunk(chunk);
        }
    }

    const pendingResponse = await active.end();
    const response = await pendingResponse.wait();
    await ensureSuccess(path, response);
    // Drain any (typically empty) body so the framework can release the
    // stream cleanly.
    await readResponseBody(response);
    return response;
}

/** Coerces a `bigint | number` value to `number` (lossy for very large i64). */
export function asNumber(value: number | bigint): number {
    return typeof value === "bigint" ? Number(value) : value;
}
