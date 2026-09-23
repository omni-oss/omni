import { defineConfig } from "@pandacss/dev";
import basePreset from "@pandacss/preset-base";
import createPandaPreset from "@pandacss/preset-panda";
import { createTypographyPreset } from "@pandacss/preset-typography";

export default defineConfig({
    presets: [
        basePreset,
        createPandaPreset,
        // This beta exports a factory (default export is a function), so the
        // bare "@pandacss/preset-typography" string can't resolve to an object.
        createTypographyPreset(),
    ], // default utilities, tokens & conditions
    preflight: true, // CSS reset
    include: ["./src/**/*.{js,jsx,ts,tsx}"],
    exclude: [],
    theme: {
        extend: {
            // Referenced by the spinning logo in routes/index.tsx. Keyframes can
            // only be declared here (the `css()` function can't emit @keyframes).
            keyframes: {
                "logo-spin": {
                    from: { transform: "rotate(0deg)" },
                    to: { transform: "rotate(360deg)" },
                },
            },
        },
    },
});
