import { Socket } from "node:net";
import { AbstractTransport } from "./abstract-transport";
import type { Transport } from "./interface";

export type TcpTransportConfig = {
    host: string;
    port: number;
};

export class TcpTransport extends AbstractTransport implements Transport {
    private readonly socket: Socket;

    constructor(private readonly config: TcpTransportConfig) {
        super();
        this.socket = new Socket();

        this.socket.on("data", this.receiveBytes);
    }

    async connect(): Promise<void> {
        this.socket.connect(this.config.port, this.config.host);
        return new Promise((resolve, reject) => {
            this.socket.once("connect", resolve);
            this.socket.once("error", reject);
        });
    }

    protected override sendBytes(data: Uint8Array): Promise<void> {
        return new Promise((resolve, reject) => {
            this.socket.write(data, (error) => {
                if (error) {
                    reject(error);
                } else {
                    resolve();
                }
            });
        });
    }
}
