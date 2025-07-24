import { LENGTH_PREFIX_LENGTH } from "./constants";

export class TransportReadFramer {
    private currentFrameBytes: Uint8Array[] = [];
    private currentExpectedFrameLengthBytes: Uint8Array[] = [];
    private currentExpectedFrameLength: number | null = null;

    private prefixBufferedLength = 0;
    private frameBufferedLength = 0;

    frame(bytes: Uint8Array): Uint8Array[] | false {
        let offset = 0;
        const frames: Uint8Array[] = [];

        while (offset < bytes.byteLength) {
            // Collect length prefix first
            if (this.currentExpectedFrameLength === null) {
                const needed = LENGTH_PREFIX_LENGTH - this.prefixBufferedLength;
                const chunk = bytes.subarray(offset, offset + needed);
                this.currentExpectedFrameLengthBytes.push(chunk);
                this.prefixBufferedLength += chunk.byteLength;
                offset += chunk.byteLength;

                if (this.prefixBufferedLength === LENGTH_PREFIX_LENGTH) {
                    const fullPrefix = this.concat(
                        this.currentExpectedFrameLengthBytes,
                    );
                    const view = new DataView(
                        fullPrefix.buffer,
                        fullPrefix.byteOffset,
                        fullPrefix.byteLength,
                    );
                    this.currentExpectedFrameLength = view.getUint32(0, true);
                    this.currentExpectedFrameLengthBytes = [];
                    this.prefixBufferedLength = 0;
                } else {
                    return false;
                }
            }

            // Now collect the frame body
            if (this.currentExpectedFrameLength !== null) {
                const needed =
                    this.currentExpectedFrameLength - this.frameBufferedLength;
                const chunk = bytes.subarray(offset, offset + needed);
                this.currentFrameBytes.push(chunk);
                this.frameBufferedLength += chunk.byteLength;
                offset += chunk.byteLength;

                if (
                    this.frameBufferedLength === this.currentExpectedFrameLength
                ) {
                    frames.push(this.concat(this.currentFrameBytes));
                    this.currentFrameBytes = [];
                    this.currentExpectedFrameLength = null;
                    this.frameBufferedLength = 0;
                }
            }
        }

        return frames.length > 0 ? frames : false;
    }

    private concat(chunks: Uint8Array[]): Uint8Array {
        const totalLength = chunks.reduce((sum, c) => sum + c.byteLength, 0);
        const result = new Uint8Array(totalLength);
        let offset = 0;
        for (const chunk of chunks) {
            result.set(chunk, offset);
            offset += chunk.byteLength;
        }
        return result;
    }
}
