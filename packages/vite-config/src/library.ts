import dts from "unplugin-dts/vite";
import {
    type BaseConfigOptions,
    createConfig as baseCreateConfig,
} from "./base.ts";

const config = createConfig();

export default config;

export type LibraryConfigOptions = BaseConfigOptions & {
    typesTsConfigPath?: string;
    bundleTypes?: Exclude<
        Parameters<typeof dts>["0"],
        undefined
    >["bundleTypes"];
};

export function createConfig(options?: LibraryConfigOptions) {
    const typesTsConfigPath =
        options?.typesTsConfigPath || "./tsconfig.types.json";
    return baseCreateConfig({
        ...options,
        externalizeDeps: options?.externalizeDeps ?? true,
        overrides: {
            ...options?.overrides,
            plugins: [
                dts({
                    pathsToAliases: true,
                    bundleTypes:
                        typeof options?.bundleTypes === "boolean"
                            ? options.bundleTypes
                                ? {
                                      extractorConfig: {
                                          compiler: {
                                              tsconfigFilePath:
                                                  typesTsConfigPath,
                                          },
                                      },
                                  }
                                : false
                            : (options?.bundleTypes ?? false),
                    tsconfigPath: typesTsConfigPath,
                }),
                ...(options?.overrides?.plugins ?? []),
            ],
        },
    });
}
