import { join } from "node:path";
import {
    type BridgeRpc,
    ResponseStatusCode,
    StreamTransport,
} from "@omni-oss/bridge-rpc-core";
import { readBody } from "@omni-oss/bridge-rpc-utils/body";
import {
    type ChildFunction,
    type EnabledFunction,
    type LeveledLogFunction,
    Log,
    type LogFunction,
    type Logger,
    type WithFunction,
} from "@omni-oss/log";
import { describe, expect, it, vi } from "vitest";
import { createRpcInstance } from "..";

const __dirname = import.meta.dirname;

const TEXT_ENCODER = new TextEncoder();
const TEXT_DECODER = new TextDecoder();

describe("integration test", {
    timeout: 10_000,
}, () => {
    it(
        "should respond to /exec-generator-script requests",
        withRpcs(async ({ rpc1: rpc, logger }) => {
            const request = await rpc.clientHandle
                .request("/exec-generator-script")
                .then((req) => req.start());
            const scriptPath = join(__dirname, "__fixtures__", "test.mjs");
            await request.writeBodyChunk(
                json([
                    {
                        path: scriptPath,
                        params: {
                            dry_run: true,
                            data: null,
                            output_dir: join(
                                __dirname,
                                "__fixtures__",
                                "output",
                            ),
                        },
                    },
                ]),
            );
            const end = await request.end().then((x) => x.wait());

            const body = await readBody(end);
            if (!end.status.equals(ResponseStatusCode.SUCCESS)) {
                console.error(
                    "Error response body:",
                    TEXT_DECODER.decode(body),
                );
            }

            expect(end.status).toEqual(ResponseStatusCode.SUCCESS);
            expect(logger.info).toHaveBeenCalledWith(
                "Hello from the generator script!",
            );
        }),
    );
});

function createRpcs(): Rpcs {
    const rctSide = new TransformStream<Uint8Array, Uint8Array>();
    const rstSide = new TransformStream<Uint8Array, Uint8Array>();

    const rct = new StreamTransport({
        input: rctSide.readable,
        output: rstSide.writable,
    });

    const rst = new StreamTransport({
        input: rstSide.readable,
        output: rctSide.writable,
    });

    const mLogger = mockLogger();

    const rpc1 = createRpcInstance(rct, {
        services: [
            {
                path: "/proc/snapshot",
                handler: async (ctx) => {
                    await readBody(ctx.request);

                    await ctx.response
                        .start(ResponseStatusCode.SUCCESS, {
                            returns: {
                                current_dir: "/home/user/test",
                                args: [],
                                env: {},
                            },
                        })
                        .then((res) => res.end());
                },
            },
        ],
    });
    const rpc2 = createRpcInstance(rst);

    return {
        rpc1,
        rpc2,
        start: () =>
            Promise.all([rpc1.start(), rpc2.start()]).then(() => void 0),
        stop: () => Promise.all([rpc1.stop(), rpc2.stop()]).then(() => void 0),
        logger: mLogger,
    };
}

type ActionContext = {
    rpc1: BridgeRpc;
    rpc2: BridgeRpc;
    logger: ReturnType<typeof mockLogger>;
};

function withRpcs(action: (ctx: ActionContext) => Promise<void>) {
    return async () => {
        const { rpc1, rpc2, start, stop, logger } = createRpcs();
        try {
            await Log.withRoot(
                { get: (_) => logger as unknown as Logger },
                ["bridge-service"],
                async () => {
                    await start();
                    await delay(10); // wait for the rpcs to be ready
                    await action({ rpc1, rpc2, logger });
                    await delay(10);
                },
            );
        } finally {
            await stop();
        }
    };
}

type Rpcs = {
    rpc1: BridgeRpc;
    rpc2: BridgeRpc;
    start: () => Promise<void>;
    stop: () => Promise<void>;
    logger: ReturnType<typeof mockLogger>;
};

function json(unknown: unknown) {
    return TEXT_ENCODER.encode(JSON.stringify(unknown));
}

function delay(ms: number) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

function mockLogger() {
    return {
        error: vi.fn<LeveledLogFunction>(),
        warn: vi.fn<LeveledLogFunction>(),
        info: vi.fn<LeveledLogFunction>(),
        debug: vi.fn<LeveledLogFunction>(),
        trace: vi.fn<LeveledLogFunction>(),
        log: vi.fn<LogFunction>(),
        child: vi.fn<ChildFunction>(),
        parent: null as unknown as Logger | null,
        enabled: vi.fn<EnabledFunction>((_) => true),
        with: vi.fn<WithFunction>().mockReturnThis(),
    };
}
