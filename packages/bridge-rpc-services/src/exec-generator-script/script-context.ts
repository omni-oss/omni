import type { ClientHandle } from "@omni-oss/bridge-rpc-core";
import { Log, type Logger } from "@omni-oss/log";
import { OptimizedSystem, type System } from "@omni-oss/system-interface";
import { BridgeRpcSystem } from "../rpc-system";

export type GeneratorScriptContextOptions = {
    dryRun: boolean;
    clientHandle: ClientHandle;
    logger?: Logger;
};

export type GeneratorScriptContext = Readonly<{
    sys: System;
    log: Logger;
    isDryRun: boolean;
}>;

export class DefaultScriptContext implements GeneratorScriptContext {
    private constructor(
        public readonly sys: System,
        public readonly log: Logger,
        public readonly isDryRun: boolean,
    ) {}

    public static async create(
        options: GeneratorScriptContextOptions,
    ): Promise<DefaultScriptContext> {
        const sys = options.dryRun
            ? await BridgeRpcSystem.create(options.clientHandle)
            : await OptimizedSystem.create();
        const log = options.logger ?? Log.instance();
        return new DefaultScriptContext(sys, log, options.dryRun);
    }
}
