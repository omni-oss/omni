import {
    type DecoderOptions,
    decode as decodeMsgPack,
    type EncoderOptions,
    encode as encodeMsgPack,
} from "@msgpack/msgpack";
import { Id } from "../id";
import { RequestErrorCode, ResponseErrorCode } from "./error-code";
import { Frame } from "./frame";
import { ResponseStatusCode } from "./status-code";

const COMMON_ENCODER_OPTIONS: EncoderOptions = {
    sortKeys: true,
    useBigInt64: true,
};

const COMMON_DECODER_OPTIONS: DecoderOptions = {
    useBigInt64: true,
};

/**
 * Strip the wrapper classes (`Id`, `ResponseStatusCode`,
 * `RequestErrorCode`, `ResponseErrorCode`) down to their primitive
 * `valueOf()` representation before the value is handed to the msgpack
 * encoder.
 *
 * This is what gives us wire compatibility with the Rust
 * `bridge_rpc_core` implementation, which serializes these types as
 * plain integers (via `#[serde(transparent)]` / `serde_repr`).
 *
 * Decoding is symmetric: msgpack returns plain primitives (bigint /
 * number / string), and the existing zod schemas (e.g. `IdSchema =
 * z.bigint().transform((v) => Id.fromBigInt(v))`) lift them back into
 * the wrapper classes — so the decode path doesn't need any extra
 * post-processing here.
 */
function unwrapWireTypes(value: unknown): unknown {
    if (value === null || value === undefined) {
        return value;
    }
    if (
        value instanceof Id ||
        value instanceof ResponseStatusCode ||
        value instanceof RequestErrorCode ||
        value instanceof ResponseErrorCode
    ) {
        return value.valueOf();
    }
    if (value instanceof Uint8Array) {
        // Body chunks pass through by reference — no copying.
        return value;
    }
    if (Array.isArray(value)) {
        return value.map(unwrapWireTypes);
    }
    if (typeof value === "object") {
        const out: Record<string, unknown> = {};
        for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
            out[k] = unwrapWireTypes(v);
        }
        return out;
    }
    return value;
}

export function encode<TData>(data: TData): Uint8Array {
    return encodeMsgPack(unwrapWireTypes(data), COMMON_ENCODER_OPTIONS);
}

export function encodeFrame(frame: Frame) {
    const encodedTup = encode(Frame.toTuple(frame));
    return encodedTup;
}

export function decode(data: Uint8Array): unknown {
    return decodeMsgPack(data, COMMON_DECODER_OPTIONS);
}

export function decodeFrame(data: Uint8Array): Frame {
    const decoded = decode(data);
    if (!Array.isArray(decoded)) {
        throw new Error(
            `Expected decoded frame to be an array, got ${typeof decoded}`,
        );
    }
    if (decoded.length !== 2) {
        throw new Error(
            `Expected decoded frame to be a 2-tuple, got array of length ${decoded.length}`,
        );
    }
    return Frame.fromTuple(decoded as [Frame["type"], unknown]);
}
