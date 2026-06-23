import e2eTestConfig from "@omni-oss/vitest-config/e2e";
import { mergeConfig, type UserWorkspaceConfig } from "vitest/config";
import baseConfig from "./vite.config";

export default mergeConfig(mergeConfig(baseConfig, e2eTestConfig), {
    test: {
        include: [
            "./src/**/*.service.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
            "./src/**/*.e2e.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
        exclude: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
            // The harness is shared infrastructure, not a test suite.
            "./src/harness/**",
        ],
        // Build the omni binary once before any worker starts.
        globalSetup: ["./src/harness/global-setup.ts"],
        // Register custom matchers (toSucceed, toOutputContaining, ...).
        setupFiles: ["./src/harness/setup.ts"],
        // Spawning a real binary (and tasks it shells out to) is slower than a
        // unit test, so give e2e tests room before timing out.
        testTimeout: 30_000,
        hookTimeout: 30_000,
        tags: [
            {
                name: "generator",
            },
            {
                name: "mcp",
            },
            {
                name: "input",
            },
            {
                name: "prompt",
            },
            {
                name: "hashing",
            },
            {
                name: "caching",
            },
            {
                name: "execution",
            },
        ],
        strictTags: true,
    },
} satisfies UserWorkspaceConfig);
