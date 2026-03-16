import dts from "unplugin-dts/vite";
import {
    type BaseConfigOptions,
    createConfig as baseCreateConfig,
} from "./base.ts";

const config = createConfig();

export default config;

export type LibraryConfigOptions = BaseConfigOptions & {
    typesTsConfigPath?: string;
};

export function createConfig(options?: LibraryConfigOptions) {
    return baseCreateConfig({
        ...options,
        externalizeDeps: options?.externalizeDeps ?? true,
        overrides: {
            ...options?.overrides,
            plugins: [
                dts({
                    tsconfigPath:
                        options?.typesTsConfigPath || "./tsconfig.types.json",
                }),
                ...(options?.overrides?.plugins ?? []),
            ],
        },
    });
}
