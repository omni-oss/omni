import type { ClientHandle } from "@omni-oss/bridge-rpc-core";
import { BridgeRpcSystem } from "@omni-oss/bridge-rpc-system-interface";
import type { EnforcedSystem } from "@omni-oss/gen-sdk-core";
import { Log, type Logger } from "@omni-oss/log";
import {
    activePolicy,
    buildEnforcedSystem,
    InMemoryCwdSystem,
} from "../exec-generator-script";

/**
 * The context handed to a tool's default-exported function.
 *
 * Mirrors the generator script context but is data-only: a tool receives its
 * validated `inputs`, a capability-enforced system handle, and a logger, and
 * returns a JSON-serializable value.
 */
export type ToolScriptContext = Readonly<{
    /** The tool's validated inputs, as a JSON object. */
    inputs: unknown;
    /** Capability-enforced system surface (fs / proc / net). */
    sys: EnforcedSystem;
    /** Ambient logger. */
    log: Logger;
}>;

export type ToolScriptContextOptions = {
    clientHandle: ClientHandle;
    inputs: unknown;
    /** Base directory relative paths resolve against (the workspace root). */
    cwd: string;
    logger?: Logger;
};

export class DefaultToolScriptContext implements ToolScriptContext {
    private constructor(
        public readonly inputs: unknown,
        public readonly sys: EnforcedSystem,
        public readonly log: Logger,
    ) {}

    public static async create(
        options: ToolScriptContextOptions,
    ): Promise<DefaultToolScriptContext> {
        // Filter `proc.env()` by the same `env` rules the shim enforces (an
        // empty, passthrough policy when the run is unconfined).
        const baseSys = await BridgeRpcSystem.create(options.clientHandle, {
            envRules: activePolicy().envRuleLayers(),
        });
        // Virtualise the current working directory so a tool can use relative
        // paths (resolved against the workspace root) without mutating the real
        // host process, and so the host broker only ever sees fully-qualified,
        // root-anchored paths.
        const cwdSys = InMemoryCwdSystem.wrap(baseSys, options.cwd);
        // Layer the capability-enforcing `net` / `proc.spawn` surface on top,
        // driven by the residual policy installed from `--enforce` at startup.
        const sys = buildEnforcedSystem(cwdSys);
        const log = options.logger ?? Log.instance();
        return new DefaultToolScriptContext(options.inputs, sys, log);
    }
}
