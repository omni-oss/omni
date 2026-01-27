import integrationTestConfig from "@omni-oss/vitest-config/integration";
import { mergeConfig, type UserWorkspaceConfig } from "vitest/config";
import baseConfig from "./vite.config";

export default mergeConfig(mergeConfig(baseConfig, integrationTestConfig), {
    test: {
        include: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
    },
} satisfies UserWorkspaceConfig);
