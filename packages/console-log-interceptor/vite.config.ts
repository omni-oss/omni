import { createConfig } from "@omni-oss/vite-config/library";

import packageJson from "./package.json";

export default createConfig({
    package: packageJson,

    overrides: {
        build: {
            lib: {
                entry: "src/index.ts",

                formats: ["es"],
                fileName: (format, entryName) =>
                    `${entryName || "log-interceptor"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "LogInterceptor",
            },
        },
    },
});
