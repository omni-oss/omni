import type { ClientHandle } from "@omni-oss/bridge-rpc-core";
import { BridgeRpcSystem } from "@omni-oss/bridge-rpc-system-interface";
import type {
    EnforcedSystem,
    GeneratorScriptContext,
} from "@omni-oss/gen-sdk-core";
import { Log, type Logger } from "@omni-oss/log";
import { activePolicy, buildEnforcedSystem } from "./enforcement";
import { InMemoryCwdSystem } from "./in-memory-cwd-system";

export type { GeneratorScriptContext };

export type GeneratorScriptContextOptions = {
    dryRun: boolean;
    /** Arbitrary, already-templated data provided by the action config. */
    data: unknown;
    clientHandle: ClientHandle;
    logger?: Logger;
    outputDir: string;
};

export class DefaultScriptContext implements GeneratorScriptContext {
    private constructor(
        public readonly sys: EnforcedSystem,
        public readonly log: Logger,
        public readonly outputDir: string,
        public readonly isDryRun: boolean,
        public readonly data: unknown,
    ) {}

    public static async create(
        options: GeneratorScriptContextOptions,
    ): Promise<DefaultScriptContext> {
        // Filter `proc.env()` by the very same `env` rules the shim enforces,
        // installed from `--enforce` at startup. When the shim does not enforce
        // `env`, `envRuleLayers()` is `undefined` and the (already
        // broker-filtered) snapshot passes through verbatim.
        const baseSys = await BridgeRpcSystem.create(options.clientHandle, {
            envRules: activePolicy().envRuleLayers(),
        });
        // Virtualise the current working directory so scripts can `cd` and use
        // relative paths (resolved against `outputDir`) without mutating the
        // real host process.
        const cwdSys = InMemoryCwdSystem.wrap(baseSys, options.outputDir);
        // Layer the capability-enforcing `net` / `proc.spawn` surface on top,
        // driven by the residual policy installed from `--enforce` at startup.
        const sys = buildEnforcedSystem(cwdSys);
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
