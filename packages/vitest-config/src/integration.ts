import { defineProject } from "vitest/config";

export default defineProject({
    test: {
        testTimeout: 1000,
        include: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
    },
});
