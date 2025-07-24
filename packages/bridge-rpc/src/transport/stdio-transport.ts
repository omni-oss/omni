import { AbstractTransport } from "./abstract-transport";
import type { Transport } from "./interface";

export type StdioTransportConfig = {
    input: ReadableStream<Uint8Array>;
    output: WritableStream<Uint8Array>;
};

export class StdioTransport extends AbstractTransport implements Transport {
    constructor(private readonly config: StdioTransportConfig) {
        super();
        config.input.pipeTo(
            new WritableStream({
                write: this.receiveBytes,
            }),
        );
    }

    protected override async sendBytes(data: Uint8Array): Promise<void> {
        await this.config.output.getWriter().write(data);
    }
}
