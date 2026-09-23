import { fileURLToPath } from "node:url";

import baseConfig from "@omni-oss/vitest-config/unit";
import { mergeConfig } from "vitest/config";

export default mergeConfig(baseConfig, {
    test: {
        globals: false,
        setupFiles: ["./vitest-setup.ts"],
        // Two projects because they need different halves of the framework:
        // component tests run in a DOM against the browser build, server-runtime
        // tests (the session suite) run in node against the real server build.
        projects: [
            {
                extends: true,
                test: {
                    name: "client",
                    environment: "jsdom",
                    include: ["src/**/*.test.tsx"],
                },
            },
            {
                extends: true,
                test: {
                    name: "server",
                    environment: "node",
                    include: ["src/server/**/*.test.ts"],
                    // Inline the framework so the aliases below decide which build
                    // loads (externalized modules resolve through node instead).
                    server: { deps: { inline: [/@solidjs[+/]web/] } },
                    alias: [
                        // The test pipeline resolves the framework's browser build even
                        // in node (the client posture applies pipeline-wide), so the
                        // main entry is pinned to the server build here.
                        {
                            find: /^@solidjs\/web$/,
                            replacement: fileURLToPath(
                                new URL(
                                    "./node_modules/@solidjs/web/dist/server.js",
                                    import.meta.url,
                                ),
                            ),
                        },
                        // Inlines the storage module so its own framework import goes
                        // through the alias above instead of node's externalized copy.
                        {
                            find: /^@solidjs\/web\/storage$/,
                            replacement: fileURLToPath(
                                new URL(
                                    "./node_modules/@solidjs/web/storage/dist/storage.js",
                                    import.meta.url,
                                ),
                            ),
                        },
                        // Tests run outside the turnkey server: the plugin's env module
                        // is stubbed with the same contract (live process.env reads).
                        {
                            find: "virtual:env/server",
                            replacement: fileURLToPath(
                                new URL(
                                    "./vitest-env-server-stub.ts",
                                    import.meta.url,
                                ),
                            ),
                        },
                    ],
                },
            },
        ],
    },
});
