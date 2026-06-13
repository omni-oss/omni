import { defineProject, mergeConfig } from "vitest/config";
import { coverage } from "./coverage.ts";

export default mergeConfig(
    defineProject({
        test: {
            include: [
                "./src/**/*.e2e.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
            ],
            exclude: [
                "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
            ],
        },
    }),
    { test: { coverage } },
);
