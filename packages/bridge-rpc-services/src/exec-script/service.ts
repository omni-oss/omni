import {
    type ClientHandle,
    ResponseStatusCode,
    type Service,
    type ServiceContext,
} from "@omni-oss/bridge-rpc-core";
import { readBodyAsJson } from "@omni-oss/bridge-rpc-utils/body";
import z from "zod";
import { importScript, type ScriptModule } from "./import";

const TEXT_ENCODER = new TextEncoder();

const STATUS_BAD_REQUEST = ResponseStatusCode.from(400);
const STATUS_INTERNAL_ERROR = ResponseStatusCode.from(500);

export type ExecScriptConfig = {
    postImport?: (
        module: LoadedScript,
        client: ClientHandle,
    ) => Promise<void> | void;
    postImportAll?: (
        modules: LoadedScript[],
        client: ClientHandle,
    ) => Promise<void> | void;
    import?: (
        spec: string,
        original: typeof importScript,
    ) => Promise<ScriptModule>;
};

export type LoadedScript = {
    path: string;
    module: ScriptModule;
};

export class ExecScript implements Service {
    constructor(private readonly config: ExecScriptConfig = {}) {}

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
            await fail(context, STATUS_BAD_REQUEST, err);
            return;
        }

        let loaded: LoadedScript[];
        try {
            loaded = await Promise.all(
                paths.map(async (path) => {
                    try {
                        const script = {
                            path,
                            module: this.config.import
                                ? await this.config.import(path, importScript)
                                : await importScript(path),
                        };
                        await this.config?.postImport?.(script, context.client);

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
            await fail(context, STATUS_INTERNAL_ERROR, err);
            return;
        }

        try {
            await this.config.postImportAll?.(loaded, context.client);
        } catch (err) {
            await fail(context, STATUS_INTERNAL_ERROR, err);
            return;
        }

        const response = await context.response.start(
            ResponseStatusCode.SUCCESS,
        );
        await response.end();
    }
}

async function fail(
    context: ServiceContext,
    status: ResponseStatusCode,
    err: unknown,
): Promise<void> {
    const response = await context.response.start(status);
    await response.writeBodyChunk(TEXT_ENCODER.encode(messageOf(err)));
    await response.end();
}

const PathsSchema = z.union([z.array(z.string()), z.string()]);

function parsePaths(body: unknown): z.infer<typeof PathsSchema> {
    const data = PathsSchema.parse(body);
    return data;
}

function messageOf(err: unknown): string {
    return err instanceof Error ? err.message : String(err);
}
