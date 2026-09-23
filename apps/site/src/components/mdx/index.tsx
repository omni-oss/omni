// The MDX component map: this is where you customize how every element renders.
//
// Two layers:
//   1. `defaults` — every standard HTML tag markdown can emit, wrapped in a
//      Solid component. This is REQUIRED, not cosmetic: with `jsx: true` the
//      MDX compiler leaves `<_components.h1>…</_components.h1>` in the output,
//      and Solid's compiler routes member-expression tags through
//      `createComponent`, which cannot render a bare string like "h1". Wrapping
//      each tag in <Dynamic component="h1"> gives it a real function component.
//   2. Overrides — swap in styled/interactive Solid components for specific
//      tags (shown here for h1, a, code, pre).
import { dynamic } from "@solidjs/web";
import { Show, type Component } from "solid-js";

import type { MDXComponents } from "../../lib/mdx-provider";

// Tags markdown + remark-gfm can produce.
const HTML_TAGS = [
    "a",
    "blockquote",
    "br",
    "code",
    "del",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "img",
    "input",
    "li",
    "ol",
    "p",
    "pre",
    "strong",
    "table",
    "tbody",
    "td",
    "th",
    "thead",
    "tr",
    "ul",
] as const;

function intrinsic(tag: string): Component<Record<string, unknown>> {
    return dynamic(() => tag);
}

// NOTE: MDX emits React-style `className` (e.g. on fenced-code `<code>`), which
// Solid renders as a literal `className` attribute rather than `class`. For a
// production setup, normalize `className` -> `class` in these components (or via
// a rehype plugin) so highlight classes land on the right attribute.

const defaults = Object.fromEntries(
    HTML_TAGS.map((tag) => [tag, intrinsic(tag)]),
) as MDXComponents;

// --- Overrides: the "customize how it renders" surface ---

const Heading1: Component<Record<string, unknown>> = (props) => (
    <h1 class="mdx-h1" {...props} />
);

const Anchor: Component<{ href?: string } & Record<string, unknown>> = (
    props,
) => {
    const external = () => /^(https?:|mailto:)/.test(props.href ?? "");
    return (
        <Show when={external()} fallback={<a class="mdx-link" {...props} />}>
            <a class="mdx-link" target="_blank" rel="noreferrer" {...props} />
        </Show>
    );
    // NOTE: for real SPA navigation, swap the internal branch for TanStack's
    // <Link to={props.href}> once routes are known.
};

const InlineCode: Component<Record<string, unknown>> = (props) => (
    <code class="mdx-code" {...props} />
);

const Pre: Component<Record<string, unknown>> = (props) => (
    <pre class="mdx-pre" {...props} />
);

export const mdxComponents: MDXComponents = {
    ...defaults,
    h1: Heading1,
    a: Anchor,
    code: InlineCode,
    pre: Pre,
};
