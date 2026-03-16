import dts from "unplugin-dts/vite";
import {
    type BaseConfigOptions,
    createConfig as baseCreateConfig,
} from "./base.ts";

export type ScriptConfigOptions = BaseConfigOptions & {
    generateTypes?: boolean;
    typesTsConfigPath?: string;
};

const config = createConfig({
    generateTypes: true,
    externalizeDeps: true,
});

export default config;

export function createConfig(options?: ScriptConfigOptions) {
    return baseCreateConfig({
        ...options,
        externalizeDeps: options?.externalizeDeps ?? true,
        overrides: {
            ...options?.overrides,
            plugins: options?.generateTypes
                ? [
                      dts({
                          tsconfigPath:
                              options.typesTsConfigPath ||
                              "./tsconfig.types.json",
                      }),
                      ...(options?.overrides?.plugins ?? []),
                  ]
                : (options?.overrides?.plugins ?? []),
        },
    });
}
