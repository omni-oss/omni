import type { ResponseStatusCode } from "@omni-oss/bridge-rpc-core";
import type { PendingResponse } from "@omni-oss/bridge-rpc-core/server";

const TEXT_ENCODER = new TextEncoder();

export async function fail(
    pendingResponse: PendingResponse,
    status: ResponseStatusCode,
    err: unknown,
): Promise<void> {
    const response = await pendingResponse.start(status);
    await response.writeBodyChunk(TEXT_ENCODER.encode(messageOf(err)));
    await response.end();
}

function messageOf(unknown: unknown): string {
    if (unknown instanceof Error) {
        return unknown.message;
    }
    try {
        return String(unknown);
    } catch {
        return "An unknown error occurred.";
    }
}
