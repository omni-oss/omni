import serviceTestConfig from "@omni-oss/vitest-config/service";
import { mergeConfig, type UserWorkspaceConfig } from "vitest/config";
import baseConfig from "./vite.config";

export default mergeConfig(mergeConfig(baseConfig, serviceTestConfig), {
    test: {
        include: [
            "./src/**/*.service.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
        exclude: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
    },
} satisfies UserWorkspaceConfig);
