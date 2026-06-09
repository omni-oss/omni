import { stderr } from "node:process";
import {
    type ClientHandle,
    ResponseStatusCode,
} from "@omni-oss/bridge-rpc-core";
import type { AsyncSink, LogLevel, LogRecord } from "@omni-oss/log/logtape";

export type BridgeRpcSinkOptions = {
    client: ClientHandle;
};

export function getBridgeRpcSink(options: BridgeRpcSinkOptions): AsyncSink {
    const sink = new BridgeRpcSink(options.client);
    return async (entry) => {
        await sink.log(entry);
    };
}

const textEncoder = new TextEncoder();
export class BridgeRpcSink {
    constructor(private client: ClientHandle) {}

    async log(entry: LogRecord) {
        try {
            const payload = {
                level: convertLevel(entry.level),
                fields: entry.properties,
                message: entry.message.join(" "),
                target: entry.category,
                timestamp: entry.timestamp,
            };

            await using response = await this.client
                .request("/log")
                .then((res) => res.start())
                .then(async (res) =>
                    res.writeBodyChunk(
                        textEncoder.encode(JSON.stringify(payload)),
                    ),
                )
                .then((res) => res.end())
                .then((res) => res.wait());

            if (!response.status.equals(ResponseStatusCode.SUCCESS)) {
                stderr.write(
                    `Failed to send log entry via Bridge RPC: Non-success status code ${response.status}\n`,
                );
                return;
            }
        } catch (error) {
            stderr.write(`Failed to send log entry via Bridge RPC: ${error}\n`);
        }
    }
}

function convertLevel(level: LogLevel) {
    switch (level) {
        case "warning":
            return "warn";
        case "fatal":
            return "error";
        default:
            return level;
    }
}
