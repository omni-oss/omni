import {
    type ClientHandle,
    ResponseStatusCode,
    type Service,
    type ServiceContext,
} from "@omni-oss/bridge-rpc-core";
import { readBodyAsJson } from "@omni-oss/bridge-rpc-utils/body";
import { fail } from "@omni-oss/bridge-rpc-utils/server";
import { flushLogs } from "@omni-oss/log";
import z from "zod";
import { importScript, type ScriptModule } from "./import";

const STATUS_BAD_REQUEST = ResponseStatusCode.from(400);
const STATUS_INTERNAL_ERROR = ResponseStatusCode.from(500);

export type ExecScriptConfig<TExtraParams = unknown> = {
    postImport?: (
        module: LoadedScript,
        client: ClientHandle,
        extraParams: TExtraParams,
    ) => Promise<void> | void;
    postImportAll?: (
        modules: LoadedScript[],
        client: ClientHandle,
        extraParams: TExtraParams,
    ) => Promise<void> | void;
    import?: (
        spec: string,
        original: typeof importScript,
        extraParams: TExtraParams,
    ) => Promise<ScriptModule>;
};

export type LoadedScript = {
    path: string;
    module: ScriptModule;
};

export class ExecScript<TExtraParams = unknown> implements Service {
    constructor(private readonly config: ExecScriptConfig<TExtraParams> = {}) {}

    public async run(context: ServiceContext) {
        let paths: string[];
        try {
            const p = parsePaths(
                await readBodyAsJson<unknown>(context.request),
            );
            if (typeof p === "string") {
                paths = [p];
            } else {
                paths = p;
            }
        } catch (err) {
            await fail(context.response, STATUS_BAD_REQUEST, err);
            return;
        }

        await this.importScripts(paths, context, {} as TExtraParams);
    }

    protected async importScripts(
        paths: string[],
        context: ServiceContext,
        extraParams: TExtraParams,
    ): Promise<void> {
        let loaded: LoadedScript[];
        try {
            loaded = await Promise.all(
                paths.map(async (path) => {
                    try {
                        const script = {
                            path,
                            module: this.config.import
                                ? await this.config.import(
                                      path,
                                      importScript,
                                      extraParams,
                                  )
                                : await importScript(path),
                        };
                        await this.config?.postImport?.(
                            script,
                            context.client,
                            extraParams,
                        );

                        return script;
                    } catch (err) {
                        // Wrap so the failing path is preserved in the message.
                        throw new Error(
                            `failed to load script "${path}": ${messageOf(err)}`,
                            { cause: err },
                        );
                    }
                }),
            );
        } catch (err) {
            await flushLogs();
            await fail(context.response, STATUS_INTERNAL_ERROR, err);
            return;
        }

        try {
            await this.config.postImportAll?.(
                loaded,
                context.client,
                extraParams,
            );
        } catch (err) {
            // Deliver any records the just-run scripts emitted before the host
            // learns the request is done and tears the connection down.
            await flushLogs();
            await fail(context.response, STATUS_INTERNAL_ERROR, err);
            return;
        }

        // Flush pending async log deliveries (e.g. the bridge `/log` RPC sink)
        // while the transport is still alive: LogTape dispatches to async sinks
        // fire-and-forget, so without this the tail of a script's log output
        // races the connection teardown that follows the response below.
        await flushLogs();
        const response = await context.response.start(
            ResponseStatusCode.SUCCESS,
        );
        await response.end();
    }
}

const PathsSchema = z.union([z.array(z.string()), z.string()]);

function parsePaths(body: unknown): z.infer<typeof PathsSchema> {
    const data = PathsSchema.parse(body);
    return data;
}

function messageOf(err: unknown): string {
    return err instanceof Error ? err.message : String(err);
}
