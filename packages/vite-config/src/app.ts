import type { UserConfig } from "vite";
import {
    type BaseConfigOptions,
    createConfig as baseCreateConfig,
} from "./base.ts";

const overrides: UserConfig = {};

export default createConfig({
    overrides,
});

export type AppConfigOptions = BaseConfigOptions;

export function createConfig(options?: AppConfigOptions) {
    return baseCreateConfig({
        ...options,
        externalizeDeps: options?.externalizeDeps ?? false,
    });
}
