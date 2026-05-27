import {
    ResponseStatusCode,
    type Service,
    type ServiceContext,
} from "@omni-oss/bridge-rpc-core";
import { readBodyAsJson } from "@omni-oss/bridge-rpc-utils/body";
import z from "zod";
import { loadScript, type ScriptModule } from "./loader";

const TEXT_ENCODER = new TextEncoder();

const STATUS_BAD_REQUEST = ResponseStatusCode.from(400);
const STATUS_INTERNAL_ERROR = ResponseStatusCode.from(500);

export type ExecScriptConfig = {
    postImport?: (module: LoadedScript) => Promise<void> | void;
    postImportAll?: (modules: LoadedScript[]) => Promise<void> | void;
};

export type LoadedScript = {
    path: string;
    module: ScriptModule;
};

export class ExecScript implements Service {
    constructor(private readonly config: ExecScriptConfig = {}) {}

    run = async (context: ServiceContext): Promise<void> => {
        let paths: string[];
        try {
            paths = parsePaths(await readBodyAsJson<unknown>(context.request));
        } catch (err) {
            await this.fail(context, STATUS_BAD_REQUEST, err);
            return;
        }

        let loaded: LoadedScript[];
        try {
            loaded = await Promise.all(
                paths.map(async (path) => {
                    try {
                        const script = { path, module: await loadScript(path) };
                        await this.config?.postImport?.(script);

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
            await this.fail(context, STATUS_INTERNAL_ERROR, err);
            return;
        }

        try {
            await this.config.postImportAll?.(loaded);
        } catch (err) {
            await this.fail(context, STATUS_INTERNAL_ERROR, err);
            return;
        }

        const response = await context.response.start(
            ResponseStatusCode.SUCCESS,
        );
        await response.end();
    };

    private async fail(
        context: ServiceContext,
        status: ResponseStatusCode,
        err: unknown,
    ): Promise<void> {
        const response = await context.response.start(status);
        await response.writeBodyChunk(TEXT_ENCODER.encode(messageOf(err)));
        await response.end();
    }
}

const PathsSchema = z.array(z.string());

function parsePaths(body: unknown): string[] {
    const data = PathsSchema.parse(body);
    return data as string[];
}

function messageOf(err: unknown): string {
    return err instanceof Error ? err.message : String(err);
}
