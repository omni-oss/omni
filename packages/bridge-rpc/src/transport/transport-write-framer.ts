import { LENGTH_PREFIX_LENGTH } from "./constants";

export class TransportWriteFramer {
    frame(data: Uint8Array): [Uint8Array, Uint8Array] {
        const lengthPrefix = new Uint8Array(LENGTH_PREFIX_LENGTH);
        new DataView(lengthPrefix.buffer).setUint32(0, data.byteLength, true);
        return [lengthPrefix, data];
    }
}
