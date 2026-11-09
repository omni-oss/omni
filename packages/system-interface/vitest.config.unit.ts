import unitTestConfig from "@omni-oss/vitest-config/unit";
import { mergeConfig, type UserWorkspaceConfig } from "vitest/config";

import baseConfig from "./vite.config";

export default mergeConfig(mergeConfig(baseConfig, unitTestConfig), {
    test: {
        typecheck: {
            // Vitest's static type-test collector cannot discover tests declared by
            // declareFsTests/declareProcTests/declareSystemTests. They still run as
            // runtime tests and remain in the TypeScript program for source checks.
            exclude: [
                "./src/{fs,proc,sys}/{bun,deno,node,optimized,virtual}.spec.ts",
            ],
        },
        include: ["./src/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}"],
        exclude: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
    },
} satisfies UserWorkspaceConfig);
