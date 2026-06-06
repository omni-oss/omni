import { createConfig } from "@omni-oss/vite-config/library";
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    overrides: {
        build: {
            lib: {
                entry: {
                    index: "./src/index.ts",
                    bun: "./src/bun.ts",
                    virtual: "./src/virtual.ts",
                    deno: "./src/deno.ts",
                    node: "./src/node.ts",
                },
                formats: ["es", "cjs"],
                fileName: (format, entryName) =>
                    `${entryName || "system-interface"}.${format === "cjs" ? "cjs" : "mjs"}`,
                name: "SystemInterface",
            },
        },
    },
});
