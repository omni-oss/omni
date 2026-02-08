import { devtools } from "@tanstack/devtools-vite";
import { tanstackStart } from "@tanstack/solid-start/plugin/vite";
import { nitro } from "nitro/vite";
import { defineConfig } from "vite";
import lucidePreprocess from "vite-plugin-lucide-preprocess";
import solidPlugin from "vite-plugin-solid";
import viteTsConfigPaths from "vite-tsconfig-paths";

export default defineConfig({
    plugins: [
        lucidePreprocess(),
        devtools(),
        // this is the plugin that enables path aliases
        viteTsConfigPaths({
            projects: ["./tsconfig.json"],
        }),
        tanstackStart({
            // prerender: {
            //     enabled: true,
            //     autoSubfolderIndex: true,
            //     crawlLinks: true,
            // },
        }),
        nitro({
            preset: "vercel",
        }),
        solidPlugin({ ssr: true }),
    ],
});
