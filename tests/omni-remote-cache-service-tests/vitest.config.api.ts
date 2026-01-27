import apiTestConfig from "@omni-oss/vitest-config/api";
import { mergeConfig, type UserWorkspaceConfig } from "vitest/config";
import baseConfig from "./vite.config";

export default mergeConfig(mergeConfig(baseConfig, apiTestConfig), {
    test: {
        include: ["./src/**/*.api.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}"],
        exclude: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
    },
} satisfies UserWorkspaceConfig);
