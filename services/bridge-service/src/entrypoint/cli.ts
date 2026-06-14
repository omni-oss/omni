import { AsyncLocalStorage } from "node:async_hooks";
import { Writable } from "node:stream";
import { Command } from "@commander-js/extra-typings";
import { interceptLogs } from "@omni-oss/console-log-interceptor";
import { Log } from "@omni-oss/log";
import { getStreamSink, withLogTapeRoot } from "@omni-oss/log/logtape";
import { createStudioRpcInstance } from "@/index";
import { getBridgeRpcSink } from "@/logging";
import { description, name, version } from "../../package.json";

const program = new Command();

program.name(name).version(version).description(description);

program.command("run").action(async () => {
    const rpc = createStudioRpcInstance();
    await withLogTapeRoot(
        ["bridge-service"],
        {
            sinks: {
                "bridge-rpc": getBridgeRpcSink({
                    get client() {
                        return rpc.clientHandle;
                    },
                }),
                stderr: getStreamSink(Writable.toWeb(process.stderr)),
            },
            loggers: [
                {
                    category: ["logtape", "meta"],
                    sinks: ["bridge-rpc"],
                    lowestLevel: "warning",
                },
                {
                    category: "bridge-service",
                    sinks: ["bridge-rpc"],
                },
                {
                    category: ["bridge-rpc-core"],
                    sinks: ["stderr"],
                    lowestLevel: "warning",
                },
            ],
            contextLocalStorage: new AsyncLocalStorage(),
        },
        async () => {
            const logger = Log.instance();
            const { logs: _l, result: _r } = await interceptLogs(
                async () => await rpc.start(),
                {
                    listeners: [
                        (entry) => {
                            logger.log(
                                entry.level,
                                typeof entry.args[0] === "string"
                                    ? entry.args[0]
                                    : String(entry.args[0]),
                                {
                                    args:
                                        entry.args.length > 1
                                            ? entry.args.slice(1)
                                            : [],
                                },
                            );
                        },
                    ],
                    max: 1,
                },
            );
        },
    );
});

await program.parseAsync();
