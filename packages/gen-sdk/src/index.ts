import type { GeneratorScriptContext } from "@omni-oss/gen-sdk-core";

export type * from "@omni-oss/gen-sdk-core";

export function defineEntrypoint(
    script: (context: GeneratorScriptContext) => Promise<void> | void,
) {
    return script;
}
