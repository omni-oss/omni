import type { ClientHandle } from "@omni-oss/bridge-rpc-core";
import { BridgeRpcSystem } from "@omni-oss/bridge-rpc-system-interface";
import { Log, type Logger } from "@omni-oss/log";
import type { System } from "@omni-oss/system-interface";

export type GeneratorScriptContextOptions = {
    dryRun: boolean;
    /** Arbitrary, already-templated data provided by the action config. */
    data: unknown;
    clientHandle: ClientHandle;
    logger?: Logger;
};

export type GeneratorScriptContext = Readonly<{
    sys: System;
    log: Logger;
    isDryRun: boolean;
    /** Arbitrary data passed from the `run-javascript` action's config. */
    data: unknown;
}>;

export class DefaultScriptContext implements GeneratorScriptContext {
    private constructor(
        public readonly sys: System,
        public readonly log: Logger,
        public readonly isDryRun: boolean,
        public readonly data: unknown,
    ) {}

    public static async create(
        options: GeneratorScriptContextOptions,
    ): Promise<DefaultScriptContext> {
        const sys = await BridgeRpcSystem.create(options.clientHandle);
        const log = options.logger ?? Log.instance();
        return new DefaultScriptContext(sys, log, options.dryRun, options.data);
    }
}
