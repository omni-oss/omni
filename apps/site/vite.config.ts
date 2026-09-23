import { fileURLToPath } from "node:url";

import mdx from "@mdx-js/rollup";
import { pandacss } from "@pandacss/vite";
import solid from "@solidjs/vite-plugin";
import { tanstackStart } from "@tanstack/solid-start/plugin/vite";
import { nitro } from "nitro/vite";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMdxFrontmatter from "remark-mdx-frontmatter";
import { defineConfig } from "vitest/config";

export default defineConfig({
    plugins: [
        // Generates src/routeTree.gen.ts from src/routes; must be registered
        // before solid(). Each route's component compiles to a lazy chunk that
        // resolves at the read point during hydration (the router commits the
        // server's matches from Solid's hydration registry before rendering), so
        // no chunk-preload manifest from TanStack's start layer is needed.
        tanstackStart({}),
        nitro({ preset: "vercel", framework: { name: "tanstack-start" } }),
        // MDX → Solid. `jsx: true` keeps JSX in the output so Solid's own
        // compiler (below) transforms it; `enforce: "pre"` guarantees this
        // runs before solid(). `providerImportSource` wires every .mdx file to
        // src/lib/mdx-provider.tsx so component overrides resolve via context.
        {
            enforce: "pre",
            ...mdx({
                jsx: true,
                providerImportSource: fileURLToPath(
                    new URL("./src/lib/mdx-provider.tsx", import.meta.url),
                ),
                remarkPlugins: [
                    remarkGfm,
                    remarkFrontmatter,
                    [remarkMdxFrontmatter, { name: "frontmatter" }],
                ],
            }),
        },
        solid({
            // Tell Solid's compiler to also transform the JSX that the MDX
            // plugin emits for .mdx modules.
            extensions: [[".mdx", { typescript: false }]],
            // start: {
            //     middleware: "./src/middleware.ts",
            //     // Per-request SSR preparation: builds this request's TanStack
            //     // router + Query cache and runs the loaders (see src/setup.tsx).
            //     // Ignored when `ssr` is false.
            //     setup: "./src/setup.tsx",
            //     // Emit dist/server/node.js: a ready-to-run Node server (static
            //     // assets + the handler). Other platforms import dist/server/server.js.
            //     node: true,
            //     // Typed env is on by convention: ./env.ts is probed automatically.
            //     // (Set `env: false` here to opt out.)
            // },
            // Set to false for a static shell + API server: pages render on the
            // client while server functions, sessions, and API routes keep working.
            ssr: true,
            // Dev-only: capture control at /__solid/diagnostics (see AGENTS.md).
            diagnostics: true,
            // The configure module runs in the handler graph before any dispatch —
            // it registers the Query single-flight collector.
            // serverFunctions: { configure: "./src/server-config.ts" },
        }),
        pandacss({ transform: true }),
    ],
    server: {
        port: 3000,
    },
    build: {
        target: "esnext",
        assetsInlineLimit: 0,
    },
});
