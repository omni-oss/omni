import { createConfig } from "@omni-oss/vite-config/library";
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    overrides: {
        build: {
            lib: {
                entry: "src/index.ts",
                formats: ["es", "cjs"],
                fileName: (format, entryName) =>
                    `${entryName || "channels"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "Channels",
            },
        },
    },
});
