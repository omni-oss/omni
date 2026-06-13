import {
    ResponseStatusCode,
    type ServiceContext,
} from "@omni-oss/bridge-rpc-core";
import { readBody } from "@omni-oss/bridge-rpc-utils/body";
import { fail } from "@omni-oss/bridge-rpc-utils/server";
import { Log } from "@omni-oss/log";
import z from "zod";
import { ExecScript } from "../exec-script";
import {
    DefaultScriptContext,
    type GeneratorScriptContext,
} from "./script-context";

export class ExecGeneratorScript extends ExecScript<ScriptInvocation[]> {
    public constructor() {
        super({
            postImportAll: async (modules, client, invocations) => {
                const invalids = modules.filter(
                    (m) =>
                        !m.module.default ||
                        typeof m.module.default !== "function",
                );

                if (invalids.length > 0) {
                    throw new Error(
                        `The following scripts do not export a default function:\n${invalids
                            .map((m) => `- ${m.path}`)
                            .join("\n")}`,
                    );
                }

                if (!Log.isInitialized()) {
                    throw new Error(
                        "Log is not initialized. Please initialize Log before executing generator scripts.",
                    );
                }

                // `modules` preserves the order of the paths handed to
                // `importScripts`, which mirrors `invocations`, so we can pair
                // each module with its own params (and per-script `data`).
                for (let i = 0; i < modules.length; i++) {
                    const m = modules[i];
                    const invocation = invocations[i];
                    if (!m || !invocation) {
                        continue;
                    }

                    const ctx = await DefaultScriptContext.create({
                        clientHandle: client,
                        dryRun: invocation.params.dry_run,
                        data: invocation.params.data,
                        logger: Log.instance(),
                    });

                    try {
                        const fn = m.module.default as (
                            context: GeneratorScriptContext,
                        ) => Promise<void> | void;

                        await fn(ctx);
                    } catch (err) {
                        throw new Error(
                            `Error executing script at ${m.path}: ${
                                err instanceof Error ? err.message : String(err)
                            }`,
                        );
                    }
                }
            },
        });
    }

    public override async run(context: ServiceContext): Promise<void> {
        const invocations = await parsePayload(context);
        if (!invocations) {
            return;
        }
        await this.importScripts(
            invocations.map((i) => i.path),
            context,
            invocations,
        );
    }
}

const TEXT_DECODER = new TextDecoder();

const ScriptParamsSchema = z.object({
    dry_run: z.boolean(),
    data: z.unknown(),
});

const ScriptInvocationSchema = z.object({
    path: z.string(),
    params: ScriptParamsSchema,
});

const ExecGeneratorScriptPayloadSchema = z.array(ScriptInvocationSchema);

export type ScriptInvocation = z.infer<typeof ScriptInvocationSchema>;

async function parsePayload(context: ServiceContext) {
    const body = JSON.parse(
        TEXT_DECODER.decode(await readBody(context.request)),
    );
    const result = ExecGeneratorScriptPayloadSchema.safeParse(body);
    if (!result.success) {
        await fail(
            context.response,
            ResponseStatusCode.from(400),
            result.error,
        );
        return undefined;
    }
    return result.data;
}
