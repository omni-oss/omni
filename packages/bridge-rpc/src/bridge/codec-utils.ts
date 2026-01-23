import {
    type DecoderOptions,
    decode as decodeMsgPack,
    type EncoderOptions,
    ExtensionCodec,
    encode as encodeMsgPack,
} from "@msgpack/msgpack";
import { Id } from "../id";
import { RequestErrorCode, ResponseErrorCode } from "./error-code";
import { ResponseStatusCode } from "./status-code";

const extensionCodec = new ExtensionCodec();

const ID_EXT_TYPE = 0x01;
const RESPONSE_STATUS_CODE_EXT_TYPE = 0x02;
const REQUEST_ERROR_CODE_EXT_TYPE = 0x03;
const RESPONSE_ERROR_CODE_EXT_TYPE = 0x04;

const COMMON_ENCODER_OPTIONS: EncoderOptions = {
    sortKeys: true,
    useBigInt64: true,
};

const COMMON_DECODER_OPTIONS: DecoderOptions = {
    useBigInt64: true,
};

extensionCodec.register({
    type: ID_EXT_TYPE,
    encode: (input) => {
        if (input instanceof Id) {
            return encodeMsgPack(input.getValue(), COMMON_ENCODER_OPTIONS);
        }

        return null;
    },
    decode: (i) => {
        return decodeMsgPack(i, COMMON_DECODER_OPTIONS);
    },
});

extensionCodec.register({
    type: RESPONSE_STATUS_CODE_EXT_TYPE,
    encode: (input) => {
        if (input instanceof ResponseStatusCode) {
            return encodeMsgPack(input.valueOf(), COMMON_ENCODER_OPTIONS);
        }

        return null;
    },
    decode: (i) => {
        return decodeMsgPack(i, COMMON_DECODER_OPTIONS);
    },
});

extensionCodec.register({
    type: REQUEST_ERROR_CODE_EXT_TYPE,
    encode: (input) => {
        if (input instanceof RequestErrorCode) {
            return encodeMsgPack(input.valueOf(), COMMON_ENCODER_OPTIONS);
        }

        return null;
    },
    decode: (i) => {
        return decodeMsgPack(i, COMMON_DECODER_OPTIONS);
    },
});

extensionCodec.register({
    type: RESPONSE_ERROR_CODE_EXT_TYPE,
    encode: (input) => {
        if (input instanceof ResponseErrorCode) {
            return encodeMsgPack(input.valueOf(), COMMON_ENCODER_OPTIONS);
        }

        return null;
    },
    decode: (i) => {
        return decodeMsgPack(i, COMMON_DECODER_OPTIONS);
    },
});

export function encode<TData>(data: TData): Uint8Array {
    return encodeMsgPack(data, {
        ...COMMON_ENCODER_OPTIONS,
        extensionCodec,
    });
}

export function decode(data: Uint8Array) {
    return decodeMsgPack(data, {
        ...COMMON_DECODER_OPTIONS,
        extensionCodec,
    });
}
