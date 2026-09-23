// Content index for a docs site, built on top of Vite's `import.meta.glob`.
//
// The tree/resolution logic is generic (`createDocCollection`) and takes just a
// `root`; it derives its scoped `.mdx` loaders and `meta.json` sidecars from a
// content-wide glob. (`import.meta.glob`'s first argument must be a static
// literal, so the glob itself can't be parameterized — but the root can.)
//
// - loaders are LAZY: `import.meta.glob` without `eager` gives one dynamic
//   import per `.mdx`, so each doc becomes its own chunk (see the <Loading>
//   boundary in routes/docs.$.tsx).
// - metas are eager but only pull the separate `meta.json` sidecars (never the
//   doc component code), used for optional Fumadocs-style ordering/labels.
import type { Component } from "solid-js";

export type Frontmatter = {
    title?: string;
    description?: string;
    version?: string;
    author?: string;
};

export type MDXModule = {
    default: Component<{ components?: Record<string, unknown> }>;
    frontmatter?: Frontmatter;
};

// Optional `meta.json` (Fumadocs-style): `title` overrides a folder's label,
// `pages` orders its direct children (by last path segment).
export type Meta = { title?: string; pages?: string[] };

export type ResolvedDoc = {
    slug: string;
    load: () => Promise<MDXModule>;
};

export type NavNode = {
    slug: string;
    label: string;
    // Whether a page exists at this slug (vs. a pure grouping folder).
    hasPage: boolean;
    children: NavNode[];
};

export type DocCollection = {
    resolveDoc: (splat: string) => ResolvedDoc | undefined;
    buildNav: () => NavNode[];
};

// `import.meta.glob`'s first argument must be a static literal, so we glob the
// whole content root once here; a collection then scopes these maps to its own
// `root`. This is what lets `createDocCollection` take just a root.
const contentLoaders = import.meta.glob<MDXModule>("/content/**/*.mdx");
const contentMetas = import.meta.glob<Meta>("/content/**/meta.json", {
    eager: true,
    import: "default",
});

function scopeToPrefix<T>(
    map: Record<string, T>,
    prefix: string,
): Record<string, T> {
    const scoped: Record<string, T> = {};
    for (const key of Object.keys(map)) {
        if (key.startsWith(prefix) && map[key]) scoped[key] = map[key];
    }
    return scoped;
}

/**
 * Builds a doc collection rooted at `root` (e.g. `"/content/docs"`). The lazy
 * `.mdx` loaders and eager `meta.json` sidecars under that root are derived
 * from the content-wide globs above.
 */
export function createDocCollection(root: string): DocCollection {
    const prefix = root.endsWith("/") ? root : `${root}/`;
    const loaders = scopeToPrefix(contentLoaders, prefix);
    const metas = scopeToPrefix(contentMetas, prefix);

    const keyToSlug = (key: string): string => {
        let slug = key.slice(prefix.length).replace(/\.mdx$/, "");
        if (slug === "index") return "";
        if (slug.endsWith("/index")) slug = slug.slice(0, -"/index".length);
        return slug;
    };

    const metaFor = (slug: string): Meta | undefined =>
        metas[`${prefix}${slug}/meta.json`];

    const labelFor = (slug: string): string =>
        metaFor(slug)?.title ?? slug.split("/").pop() ?? slug;

    // Maps a URL splat to a doc: `foo/bar` -> `foo/bar.mdx` or
    // `foo/bar/index.mdx` (and `""` -> `index.mdx`).
    const resolveDoc = (splat: string): ResolvedDoc | undefined => {
        const clean = splat.replace(/^\/+|\/+$/g, "");
        const candidates =
            clean === ""
                ? [`${prefix}index.mdx`]
                : [`${prefix}${clean}.mdx`, `${prefix}${clean}/index.mdx`];

        for (const key of candidates) {
            if (Object.hasOwn(loaders, key) && loaders[key]) {
                return { slug: clean, load: loaders[key] };
            }
        }
        return undefined;
    };

    const sortTree = (node: NavNode): void => {
        const order = metaFor(node.slug)?.pages;
        node.children.sort((a, b) => {
            if (order) {
                const rank = (n: NavNode) => {
                    const seg = n.slug.split("/").pop() ?? "";
                    const i = order.indexOf(seg);
                    return i === -1 ? Number.MAX_SAFE_INTEGER : i;
                };
                const diff = rank(a) - rank(b);
                if (diff !== 0) return diff;
            }
            return a.label.localeCompare(b.label);
        });
        node.children.forEach(sortTree);
    };

    // Builds the sidebar tree from the file layout. Directories with an
    // `index.mdx` are linkable; directories without one are grouping labels.
    const buildNav = (): NavNode[] => {
        const nodes = new Map<string, NavNode>();

        const ensure = (slug: string): NavNode => {
            let node = nodes.get(slug);
            if (!node) {
                node = {
                    slug,
                    label: labelFor(slug),
                    hasPage: false,
                    children: [],
                };
                nodes.set(slug, node);
            }
            return node;
        };

        for (const key of Object.keys(loaders)) {
            const slug = keyToSlug(key);
            if (slug === "") continue; // root index isn't a sidebar entry
            ensure(slug).hasPage = true;
            const segments = slug.split("/");
            for (let i = 1; i < segments.length; i++) {
                ensure(segments.slice(0, i).join("/"));
            }
        }

        const roots: NavNode[] = [];
        for (const node of nodes.values()) {
            const segments = node.slug.split("/");
            if (segments.length === 1) {
                roots.push(node);
            } else {
                const parent = nodes.get(segments.slice(0, -1).join("/"));
                parent?.children.push(node);
            }
        }

        const virtualRoot: NavNode = {
            slug: "",
            label: "",
            hasPage: false,
            children: roots,
        };
        sortTree(virtualRoot);
        return virtualRoot.children;
    };

    return { resolveDoc, buildNav };
}

// The one concrete collection for this app.
const docs = createDocCollection("/content/docs");

export const resolveDoc = docs.resolveDoc;
export const buildNav = docs.buildNav;
