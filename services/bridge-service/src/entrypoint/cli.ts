import { AsyncLocalStorage } from "node:async_hooks";
import { Writable } from "node:stream";
import { Command } from "@commander-js/extra-typings";
import {
    CapabilityPolicy,
    installGlobalEnforcement,
} from "@omni-oss/bridge-rpc-services/exec-generator-script";
import { interceptLogs } from "@omni-oss/console-log-interceptor";
import { Log, registerLogDrain } from "@omni-oss/log";
import {
    fromAsyncSink,
    getStreamSink,
    withLogTapeRoot,
} from "@omni-oss/log/logtape";
import { createStudioRpcInstance } from "@/index";
import { getBridgeRpcSink } from "@/logging";
import { description, name, version } from "../../package.json";

const program = new Command();

program.name(name).version(version).description(description);

program
    .command("run")
    // The residual capability policy (JSON) the spawning host could not confine
    // via the runtime's launch flags. Installed process-wide before the RPC
    // starts so global `fetch` and `ctx.sys` enforcement are active for every
    // script this bridge executes. Absent → the runtime flags are the only
    // enforcement (an empty, passthrough policy).
    .option(
        "--enforce <json>",
        "capability residual policy to enforce at the script level (JSON)",
    )
    .action(async (options) => {
        installGlobalEnforcement(CapabilityPolicy.parse(options.enforce));
        const rpc = createStudioRpcInstance();
        // LogTape never awaits an async sink, so wrap the `/log` RPC sink with
        // `fromAsyncSink`: records are serialised onto a single tail promise
        // and the sink exposes `Symbol.asyncDispose`, which awaits that tail.
        // Registering it as a drain lets services flush pending `/log`
        // deliveries (via `flushLogs`) while the transport is still alive —
        // otherwise in-flight log requests are dropped when the host tears the
        // connection down after a script finishes.
        const bridgeRpcSink = fromAsyncSink(
            getBridgeRpcSink({
                get client() {
                    return rpc.clientHandle;
                },
            }),
        );
        registerLogDrain(async () => {
            await bridgeRpcSink[Symbol.asyncDispose]();
        });
        await withLogTapeRoot(
            ["bridge-service"],
            {
                sinks: {
                    "bridge-rpc": bridgeRpcSink,
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
