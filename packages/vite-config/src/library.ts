import dts from "unplugin-dts/vite";
import { mergeConfig, type UserConfig } from "vite";
import base from "./base.ts";

export default mergeConfig(base, {
    plugins: [dts()],
} satisfies UserConfig);
