import { createFileRoute } from "@tanstack/solid-router";
import { Loading, Show, createMemo, lazy } from "solid-js";

import { css } from "../../styled-system/css";
import { DocsSidebar } from "../components/docs-sidebar";
import { mainStyles } from "../components/main";
import { mdxComponents } from "../components/mdx";
import { resolveDoc } from "../lib/docs";
import { MDXProvider } from "../lib/mdx-provider";

const docsLayout = css({
    display: "grid",
    gridTemplateColumns: "16rem minmax(0, 1fr)",
    gap: "1rem",
    alignItems: "start",
    textAlign: "left",
});

// Reuses the shared `main` styles (link + focus treatment), then overrides the
// padding and adds the MDX code styling. Merging in one `css()` call keeps the
// padding override deterministic, matching the original `.mdx-content` win.
const mdxContent = css(mainStyles, {
    padding: "1.5rem",
    minWidth: 0,
    "& .mdx-pre": {
        padding: "0.85rem 1rem",
        overflowX: "auto",
        borderRadius: "0.5rem",
        backgroundColor: "#0f172a",
        color: "#e2e8f0",
    },
    "& .mdx-pre .mdx-code": {
        background: "none",
        padding: 0,
        color: "inherit",
    },
    "& .mdx-code": {
        padding: "0.1rem 0.35rem",
        borderRadius: "0.3rem",
        backgroundColor: "#f1f5f9",
        fontSize: "0.9em",
    },
});

export const Route = createFileRoute("/docs/$")({
    component: DocsPage,
});

function DocsPage() {
    const params = Route.useParams();

    // Each doc is code-split: `lazy()` triggers the dynamic import on first
    // render and suspends through the <Loading> boundary while the chunk loads
    // (Solid v2's replacement for <Suspense>). Recreated per slug.
    const Doc = createMemo(() => {
        const entry = resolveDoc(params()._splat ?? "");
        return entry ? lazy(entry.load) : undefined;
    });

    return (
        <div class={docsLayout}>
            <DocsSidebar />
            <main class={mdxContent}>
                <Show when={Doc()} fallback={<h1>Doc not found</h1>} keyed>
                    {(LazyDoc) => (
                        <Loading fallback={<p>Loading…</p>}>
                            <MDXProvider components={mdxComponents}>
                                <LazyDoc components={mdxComponents} />
                            </MDXProvider>
                        </Loading>
                    )}
                </Show>
            </main>
        </div>
    );
}
