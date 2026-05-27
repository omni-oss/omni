import type { Response } from "@omni-oss/bridge-rpc-core/client";
import type { Request } from "@omni-oss/bridge-rpc-core/server";

export async function readBody(request: Request | Response) {
    const chunks: Uint8Array<ArrayBufferLike>[] = [];
    for await (const chunk of request.readBody()) {
        chunks.push(chunk);
    }

    if (chunks.length === 1) {
        return chunks[0] as Uint8Array<ArrayBufferLike>;
    } else {
        return combine(chunks);
    }
}

export async function readBodyAsText(
    request: Request | Response,
): Promise<string> {
    const body = await readBody(request);
    return new TextDecoder().decode(body);
}

export async function readBodyAsJson<T>(
    request: Request | Response,
): Promise<T> {
    const text = await readBodyAsText(request);
    return JSON.parse(text) as T;
}

export function combine(bytes: Uint8Array[]): Uint8Array {
    const combined = new Uint8Array(
        bytes.reduce((sum, b) => sum + b.byteLength, 0),
    );

    let offset = 0;
    for (const byte of bytes) {
        combined.set(byte, offset);
        offset += byte.byteLength;
    }

    return combined;
}
