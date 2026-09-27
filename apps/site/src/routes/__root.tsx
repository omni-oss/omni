import { css } from "$css";
import {
    HeadContent,
    Link,
    Outlet,
    createRootRouteWithContext,
} from "@tanstack/solid-router";
import { TanStackRouterDevtools } from "@tanstack/solid-router-devtools";
import { Loading } from "solid-js";

import type { RouterContext } from "#router";

const siteNav = css({
    padding: "1rem",
    backgroundColor: "#282c34",
    "& > a": {
        display: "inline-block",
        margin: "0 0.125rem",
        padding: "0.4rem 0.75rem",
        borderRadius: "0.5rem",
        color: "#93c5fd",
        fontWeight: 600,
        textDecoration: "none",
        transition: "background-color 150ms ease, color 150ms ease",
    },
    "& > a:hover": {
        backgroundColor: "rgb(255 255 255 / 10%)",
        color: "#dbeafe",
    },
    "& > a:focus-visible": {
        outline: "3px solid #0284c7",
        outlineOffset: "3px",
        borderRadius: "0.2rem",
    },
});

// The root route: the site-wide layout every route renders inside, plus the
// not-found boundary. Declaring the RouterContext type here is what types
// `context` in every loader below. <HeadContent /> renders whatever the
// matched routes declare in their `head` options (titles here).
export const Route = createRootRouteWithContext<RouterContext>()({
    head: () => ({ meta: [{ title: "Solid App" }] }),
    component: () => (
        <>
            <HeadContent />
            <nav class={siteNav}>
                <Link to="/">Home</Link>
                <Link
                    to="/docs/$"
                    params={{ _splat: "references/commands/omni" }}
                >
                    Docs
                </Link>
            </nav>
            <Loading>
                <Outlet />
            </Loading>
            <TanStackRouterDevtools />
        </>
    ),
    notFoundComponent: () => (
        <main>
            <h1>Page Not Found</h1>
            <p>
                Visit{" "}
                <a
                    href="https://docs.solidjs.com"
                    target="_blank"
                    rel="noreferrer"
                >
                    docs.solidjs.com
                </a>{" "}
                to learn how to build Solid apps.
            </p>
        </main>
    ),
});
