import {
    ResponseStatusCode,
    type Service,
    type ServiceContext,
} from "@omni-oss/bridge-rpc-core";
import { readBody } from "@omni-oss/bridge-rpc-utils/body";
import { fail } from "@omni-oss/bridge-rpc-utils/server";
import { flushLogs, Log } from "@omni-oss/log";
import z from "zod";
import { importScript } from "../exec-script/import";
import { DefaultToolScriptContext } from "./tool-script-context";

const STATUS_BAD_REQUEST = ResponseStatusCode.from(400);
const STATUS_INTERNAL_ERROR = ResponseStatusCode.from(500);
const TEXT_DECODER = new TextDecoder();
const TEXT_ENCODER = new TextEncoder();

const ExecToolPayloadSchema = z.object({
    /** Absolute path to the tool's JavaScript entrypoint. */
    path: z.string(),
    /** Base directory relative paths resolve against (the workspace root). */
    cwd: z.string(),
    /** The tool's already-validated inputs. */
    inputs: z.unknown(),
});

export type ExecToolPayload = z.infer<typeof ExecToolPayloadSchema>;

/**
 * Executes a single tool: imports its entrypoint, invokes the default-exported
 * function with a {@link ToolScriptContext}, and writes the function's return
 * value into the response body as JSON. This return-value capture is what
 * distinguishes a tool from a generator script (which ends with an empty body).
 */
export class ExecTool implements Service {
    public async run(context: ServiceContext): Promise<void> {
        const payload = await parsePayload(context);
        if (!payload) {
            return;
        }

        let module: Record<string, unknown> & { default?: unknown };
        try {
            module = await importScript(payload.path);
        } catch (err) {
            await flushLogs();
            await fail(
                context.response,
                STATUS_INTERNAL_ERROR,
                new Error(
                    `failed to load tool "${payload.path}": ${messageOf(err)}`,
                    { cause: err },
                ),
            );
            return;
        }

        if (!module.default || typeof module.default !== "function") {
            await flushLogs();
            await fail(
                context.response,
                STATUS_INTERNAL_ERROR,
                new Error(
                    `tool "${payload.path}" does not export a default function`,
                ),
            );
            return;
        }

        if (!Log.isInitialized()) {
            await fail(
                context.response,
                STATUS_INTERNAL_ERROR,
                new Error(
                    "Log is not initialized. Please initialize Log before executing tools.",
                ),
            );
            return;
        }

        let result: unknown;
        try {
            const ctx = await DefaultToolScriptContext.create({
                clientHandle: context.client,
                inputs: payload.inputs,
                cwd: payload.cwd,
                logger: Log.instance(),
            });
            const fn = module.default as (ctx: unknown) => unknown;
            result = await fn(ctx);
        } catch (err) {
            await flushLogs();
            await fail(
                context.response,
                STATUS_INTERNAL_ERROR,
                new Error(
                    `Error executing tool at ${payload.path}: ${messageOf(err)}`,
                    { cause: err },
                ),
            );
            return;
        }

        // Flush pending async log deliveries while the transport is still alive.
        await flushLogs();
        const response = await context.response.start(
            ResponseStatusCode.SUCCESS,
        );
        await response.writeBodyChunk(
            TEXT_ENCODER.encode(JSON.stringify(result ?? null)),
        );
        await response.end();
    }
}

async function parsePayload(
    context: ServiceContext,
): Promise<ExecToolPayload | undefined> {
    let body: unknown;
    try {
        body = JSON.parse(TEXT_DECODER.decode(await readBody(context.request)));
    } catch (err) {
        await fail(context.response, STATUS_BAD_REQUEST, err);
        return undefined;
    }
    const result = ExecToolPayloadSchema.safeParse(body);
    if (!result.success) {
        await fail(context.response, STATUS_BAD_REQUEST, result.error);
        return undefined;
    }
    return result.data;
}

function messageOf(err: unknown): string {
    return err instanceof Error ? err.message : String(err);
}
