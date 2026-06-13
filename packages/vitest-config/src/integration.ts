import { defineProject, mergeConfig } from "vitest/config";
import { coverage } from "./coverage.ts";

export default mergeConfig(
    defineProject({
        test: {
            testTimeout: 1000,
            include: [
                "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
            ],
        },
    }),
    { test: { coverage } },
);
