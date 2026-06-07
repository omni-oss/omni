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

export class ExecGeneratorScript extends ExecScript<Params> {
    public constructor() {
        super({
            postImportAll: async (modules, client, params) => {
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

                const ctx = await DefaultScriptContext.create({
                    clientHandle: client,
                    dryRun: params.dry_run,
                    logger: Log.instance(),
                });
                for (const m of modules) {
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
        const payload = await parsePayload(context);
        if (!payload) {
            return;
        }
        await this.importScripts(payload.paths, context, payload.params);
    }
}

const TEXT_DECODER = new TextDecoder();

const ParamsSchema = z.object({
    dry_run: z.boolean(),
});

type Params = z.infer<typeof ParamsSchema>;

const ExecGeneratorScriptPayloadSchema = z.object({
    paths: z.array(z.string()),
    params: ParamsSchema,
});

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
