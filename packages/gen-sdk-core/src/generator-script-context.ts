import type { Logger } from "@omni-oss/log";
import type { EnforcedSystem } from "./enforced-system";

export type GeneratorScriptContext = Readonly<{
    sys: EnforcedSystem;
    log: Logger;
    isDryRun: boolean;
    outputDir: string;
    /** Arbitrary data passed from the `run-javascript` action's config. */
    data: unknown;
}>;
