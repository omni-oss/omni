import type { JSX } from "@solidjs/web";
// A minimal, Solid 2.0-native MDX components provider.
//
// `@mdx-js/rollup` is configured with `providerImportSource` pointing at this
// module, so every compiled `.mdx` file imports `useMDXComponents` from here
// and calls it to discover which component renders each element/tag. That is
// the entire "customize how MDX renders" surface — see src/components/mdx.
//
// This is deliberately hand-rolled (~20 lines) instead of depending on the
// `solid-mdx`/`solid-jsx` packages, which are unmaintained and target Solid 1.
import { createContext, useContext, type Component } from "solid-js";

export type MDXComponents = Record<string, Component<any>>;

const MDXContext = createContext<MDXComponents>({});

// Called by every compiled MDX module (as `_provideComponents`). MDX invokes it
// with no arguments during render; the optional `components` arg mirrors the
// @mdx-js/react contract for callers that want to merge inline overrides.
export function useMDXComponents(components?: MDXComponents): MDXComponents {
    const context = useContext(MDXContext);
    if (!components) return context;
    return { ...context, ...components };
}

export function MDXProvider(props: {
    components?: MDXComponents;
    children?: JSX.Element;
}): JSX.Element {
    const parent = useContext(MDXContext);
    // Solid v2: a context object IS its provider component — render it directly
    // as <MDXContext value={...}> (there is no `.Provider`).
    //
    // The lint note about "value read once" is expected and fine here: the
    // components map is static for the lifetime of a provider, so there is no
    // signal/store to keep live.
    return (
        <MDXContext
            value={{
                ...parent,
                // oxlint-disable-next-line solid/reactivity
                ...props.components,
            }}
        >
            {props.children}
        </MDXContext>
    );
}
