import { mergeConfig, type UserWorkspaceConfig } from "vitest/config";
import baseConfig from "./vite.config";
import unitTestConfig from "@omni-oss/vitest-config/unit";

export default mergeConfig(mergeConfig(baseConfig, unitTestConfig), {
    test: {
        testTimeout: 1000,
        include: ["./src/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}"],
        exclude: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
    },
} satisfies UserWorkspaceConfig);
