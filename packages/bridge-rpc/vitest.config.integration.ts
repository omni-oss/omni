import { mergeConfig, type UserWorkspaceConfig } from "vitest/config";
import baseConfig from "./vite.config";

export default mergeConfig(baseConfig, {
    test: {
        include: ["./tests/**/*.{js,mjs,cjs,ts,mts,cts,jsx,tsx}"],
    },
} satisfies UserWorkspaceConfig);
