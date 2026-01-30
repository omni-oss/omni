import path from "node:path";
import type { System } from "@omni-oss/system-interface";
import { deserialize } from "./codec-utils";
import { type SetVersionConfig, SetVersionConfigSchema } from "./config";

export const CONFIG_FILE_NAMES = [
    "set-version.yaml",
    "set-version.yml",
    "set-version.toml",
    "set-version.json",
    "set-version.jsonc",
];

export async function findConfigAtDir<TRequired extends boolean>(
    startDir: string,
    required: TRequired,
    system: System,
): Promise<
    TRequired extends true ? SetVersionConfig : SetVersionConfig | undefined
> {
    let currentDir = path.resolve(startDir);

    while ((await system.fs.isDirectory(currentDir)) && !isAtRoot(currentDir)) {
        for (const configName of CONFIG_FILE_NAMES) {
            const configPath = path.join(currentDir, configName);

            if (
                (await system.fs.pathExists(configPath)) &&
                (await system.fs.isFile(configPath))
            ) {
                const read = await system.fs.readFileAsString(configPath);
                const config = deserialize(configPath, read);
                const parsed = SetVersionConfigSchema.safeParse(config);

                if (parsed.success) {
                    return parsed.data;
                } else {
                    throw new InvalidConfigError(
                        configPath,
                        parsed.error.message,
                    );
                }
            }
        }

        currentDir = path.dirname(currentDir);
    }

    if (required) {
        throw new NoConfigFoundError(startDir);
    } else {
        // biome-ignore lint/suspicious/noExplicitAny: escape hatch
        return undefined as any;
    }
}

export class NoConfigFoundError extends Error {
    constructor(public readonly dir?: string) {
        if (dir) {
            super(`No config found in ${dir}`);
        } else {
            super("No config found");
        }
        super.name = this.constructor.name;
    }
}

export class InvalidConfigError extends Error {
    constructor(
        public readonly filePath: string,
        message: string,
    ) {
        super(`Invalid config at ${filePath}: ${message}`);
        super.name = this.constructor.name;
    }
}

function isAtRoot(dir: string) {
    return path.parse(path.resolve(dir)).root === dir;
}
