import { defineProject } from "vitest/config";

export default defineProject({
    test: {
        include: ["./src/**/*.e2e.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}"],
        exclude: [
            "./src/**/__tests__/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}",
        ],
    },
});
