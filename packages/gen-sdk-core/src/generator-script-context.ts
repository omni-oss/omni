import type { Logger } from "@omni-oss/log";
import type { System } from "@omni-oss/system-interface";

export type GeneratorScriptContext = Readonly<{
    sys: System;
    log: Logger;
    isDryRun: boolean;
    outputDir: string;
    /** Arbitrary data passed from the `run-javascript` action's config. */
    data: unknown;
}>;
