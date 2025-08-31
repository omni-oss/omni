import { defineConfig, defineDocs } from "fumadocs-mdx/config";

export const docs = defineDocs({
    dir: "content/docs",
    docs: {
        async: false,
    },
});

export default defineConfig({
    lastModifiedTime: "git",
});
