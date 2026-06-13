import type { ClientHandle } from "@omni-oss/bridge-rpc-core";
import { BridgeRpcSystem } from "@omni-oss/bridge-rpc-system-interface";
import { Log, type Logger } from "@omni-oss/log";
import type { System } from "@omni-oss/system-interface";

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
        const sys = await BridgeRpcSystem.create(options.clientHandle);
        const log = options.logger ?? Log.instance();
        return new DefaultScriptContext(sys, log, options.dryRun);
    }
}
