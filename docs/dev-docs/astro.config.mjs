// @ts-check

import starlight from "@astrojs/starlight";
import { defineConfig } from "astro/config";
import starlightThemeGalaxy from "starlight-theme-galaxy";

// https://astro.build/config
export default defineConfig({
    integrations: [
        starlight({
            plugins: [starlightThemeGalaxy()],
            title: "My Docs",
            social: [
                {
                    icon: "github",
                    label: "GitHub",
                    href: "https://github.com/omni-oss/omni",
                },
            ],
            sidebar: [
                {
                    label: "Guides",
                    items: [
                        // Each item here is one entry in the navigation menu.
                        { label: "Example Guide", slug: "guides/example" },
                    ],
                },
                {
                    label: "Reference",
                    autogenerate: { directory: "reference" },
                },
            ],
        }),
    ],
});
