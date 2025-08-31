import { loader } from "fumadocs-core/source";
import * as icons from "lucide-static";
import { create, docs } from "../../source.generated";

export const source = loader({
    source: await create.sourceAsync(docs.doc, docs.meta),
    baseUrl: "/docs",
    icon(icon) {
        if (!icon || !(icon in icons)) {
            return;
        }

        // biome-ignore lint/performance/noDynamicNamespaceImportAccess: false
        return icons[icon as keyof typeof icons];
    },
});
