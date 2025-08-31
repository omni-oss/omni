import { createFromSource } from "fumadocs-core/search/server";
import { source } from "@/lib/source";

export const searchServer = createFromSource(source, {
    // https://docs.orama.com/docs/orama-js/supported-languages
    language: "english",
    buildIndex(page) {
        return {
            id: page.url,
            title: page.data.title,
            url: page.url,
            description: page.data.description,
            structuredData: page.data.structuredData,
        };
    },
});
