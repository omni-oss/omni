import { createConfig } from "@omni-oss/vite-config/app";
import { devtools } from "@tanstack/devtools-vite";
import { tanstackStart } from "@tanstack/solid-start/plugin/vite";
import { nitro } from "nitro/vite";
import lucidePreprocess from "vite-plugin-lucide-preprocess";
import solidPlugin from "vite-plugin-solid";
import packageJson from "./package.json";

export default createConfig({
    package: packageJson,
    overrides: {
        plugins: [
            lucidePreprocess(),
            devtools(),
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
    },
});
