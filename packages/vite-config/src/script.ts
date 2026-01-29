import dts from "unplugin-dts/vite";
import { mergeConfig, type UserConfig } from "vite";
import base from "./base.ts";

export type ConfigOptions = {
    generateTypes?: boolean;
};

const config = (options: ConfigOptions = {}) =>
    mergeConfig(base, {
        plugins: [
            options.generateTypes &&
                dts({ tsconfigPath: "./tsconfig.types.json" }),
        ].filter(Boolean),
    } satisfies UserConfig);

export default config;
