// Compiled by @mdx-js/rollup — each .mdx module default-exports a Solid
// component and (via remark-mdx-frontmatter) a `frontmatter` object.
declare module "*.mdx" {
    import type { Component } from "solid-js";

    const MDXContent: Component<{
        components?: Record<string, Component<any>>;
    }>;
    export default MDXContent;
    export const frontmatter: Record<string, unknown>;
}
