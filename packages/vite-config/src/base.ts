import type { UserConfig } from "vite";
import tsconfig from "vite-tsconfig-paths";

const config: UserConfig = {
    plugins: [tsconfig()],
};

export default config;
