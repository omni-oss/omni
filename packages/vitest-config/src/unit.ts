import { defineProject, mergeConfig } from "vitest/config";

import { coverage } from "./coverage.ts";

export default mergeConfig(
    defineProject({
        test: {
            testTimeout: 1000,
            include: ["./src/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}"],
            exclude: [
                "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
            ],
            typecheck: {
                enabled: true,
                include: ["./src/**/*.{test,spec}.{ts,mts,cts,tsx}"],
                exclude: ["**/node_modules/**"],
            },
        },
    }),
    { test: { coverage } },
);
