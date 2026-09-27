import { fileURLToPath } from "node:url";

import mdx from "@mdx-js/rollup";
import pandacss from "@pandacss/vite";
import { serverFunctions } from "@solidjs/prerender/integration";
import solid from "@solidjs/vite-plugin";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import { routePathFromFile } from "filesystem-routing";
import { fileRoutes } from "filesystem-routing/vite";
import { nitro } from "nitro/vite";
import { prerender } from "prerender-crawler/vite";
import remarkFrontmatter from "remark-frontmatter";
import remarkGfm from "remark-gfm";
import remarkMdxFrontmatter from "remark-mdx-frontmatter";
import { defineConfig } from "vitest/config";

export default defineConfig({
    resolve: {
        // support tsconfig paths
        tsconfigPaths: true,
    },
    // Turnkey streaming SSR under a third-party router: no index.html and no
    // entry files — the plugin generates the entries around src/App.tsx,
    // wrapped in src/Document.tsx. `vite build` emits client assets to
    // dist/client and the request handler to dist/server, plus the Node server
    // (dist/server/node.js) `npm start` runs.
    plugins: [
        // Generates src/routeTree.gen.ts from src/routes; must be registered
        // before solid(). Each route's component compiles to a lazy chunk that
        // resolves at the read point during hydration (the router commits the
        // server's matches from Solid's hydration registry before rendering), so
        // no chunk-preload manifest from TanStack's start layer is needed.
        tanstackRouter({ target: "solid", autoCodeSplitting: true }),
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
            extensions: [[".mdx", { typescript: false }]],
            start: {
                middleware: "./src/middleware.ts",
                // Per-request SSR preparation: builds this request's TanStack
                // router + Query cache and runs the loaders (see src/setup.tsx).
                // Ignored when `ssr` is false.
                setup: "./src/setup.tsx",
                // Emit dist/server/node.js: a ready-to-run Node server (static
                // assets + the handler). Other platforms import dist/server/server.js.
                node: true,
                // Typed env is on by convention: ./env.ts is probed automatically.
                // (Set `env: false` here to opt out.)
            },
            // Set to false for a static shell + API server: pages render on the
            // client while server functions, sessions, and API routes keep working.
            ssr: true,
            // Dev-only: capture control at /__solid/diagnostics (see AGENTS.md).
            diagnostics: true,
            // The configure module runs in the handler graph before any dispatch —
            // it registers the Query single-flight collector.
            serverFunctions: { configure: "./src/server-config.ts" },
        }),
        prerender({ mode: "static", integrations: [serverFunctions()] }),
        // API routes only — TanStack owns src/routes, so the file-system router
        // scans src/api instead and mounts every module under /api.
        fileRoutes({
            dir: "src/api",
            httpMethods: true,
            toPath: (file) => "/api" + routePathFromFile(file),
        }),
        pandacss({ transform: true }),
        nitro({ preset: "vercel" }),
    ],
    server: {
        port: 3000,
    },
    build: {
        target: "esnext",
        assetsInlineLimit: 0,
    },
});
