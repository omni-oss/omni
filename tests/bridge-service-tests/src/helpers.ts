export function delay(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder();

export const TEXT = {
    decode(data: Uint8Array) {
        return TEXT_DECODER.decode(data);
    },
    encode(str: string) {
        return TEXT_ENCODER.encode(str);
    },
};

export function json(unknown: unknown) {
    return TEXT_ENCODER.encode(JSON.stringify(unknown));
}
