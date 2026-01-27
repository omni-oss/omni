import { mergeConfig, type UserWorkspaceConfig } from "vitest/config";
import baseConfig from "./vite.config";
import integrationTestConfig from "@omni-oss/vitest-config/integration";

export default mergeConfig(mergeConfig(baseConfig, integrationTestConfig), {
    test: {
        testTimeout: 1000,
        include: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
    },
} satisfies UserWorkspaceConfig);
