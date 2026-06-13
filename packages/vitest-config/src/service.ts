import { defineProject, mergeConfig } from "vitest/config";
import { coverage } from "./coverage.ts";

export default mergeConfig(
    defineProject({
        test: {
            include: [
                "./src/**/*.service.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
            ],
            exclude: [
                "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
            ],
        },
    }),
    { test: { coverage } },
);
