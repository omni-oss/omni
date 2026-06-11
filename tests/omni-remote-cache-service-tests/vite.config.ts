import { createConfig } from "@omni-oss/vite-config/test";
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    overrides: {
        build: {
            lib: {
                entry: "src/index.ts",
                formats: ["es"],
                fileName: (format) =>
                    `orcs-api-test.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "OrcsApiTest",
            },
        },
    },
});
