import type { ClientHandle } from "@omni-oss/bridge-rpc-core";
import { BridgeRpcSystem } from "@omni-oss/bridge-rpc-system-interface";
import { Log, type Logger } from "@omni-oss/log";
import type { System } from "@omni-oss/system-interface";
import { InMemoryCwdSystem } from "./in-memory-cwd-system";

export type GeneratorScriptContextOptions = {
    dryRun: boolean;
    /** Arbitrary, already-templated data provided by the action config. */
    data: unknown;
    clientHandle: ClientHandle;
    logger?: Logger;
    outputDir: string;
};

export type GeneratorScriptContext = Readonly<{
    sys: System;
    log: Logger;
    isDryRun: boolean;
    outputDir: string;
    /** Arbitrary data passed from the `run-javascript` action's config. */
    data: unknown;
}>;

export class DefaultScriptContext implements GeneratorScriptContext {
    private constructor(
        public readonly sys: System,
        public readonly log: Logger,
        public readonly outputDir: string,
        public readonly isDryRun: boolean,
        public readonly data: unknown,
    ) {}

    public static async create(
        options: GeneratorScriptContextOptions,
    ): Promise<DefaultScriptContext> {
        const baseSys = await BridgeRpcSystem.create(options.clientHandle);
        // Virtualise the current working directory so scripts can `cd` and use
        // relative paths (resolved against `outputDir`) without mutating the
        // real host process.
        const sys = InMemoryCwdSystem.wrap(baseSys, options.outputDir);
        const log = options.logger ?? Log.instance();
        return new DefaultScriptContext(
            sys,
            log,
            options.outputDir,
            options.dryRun,
            options.data,
        );
    }
}
