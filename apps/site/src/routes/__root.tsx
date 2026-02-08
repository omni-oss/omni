import {
    createRootRouteWithContext,
    HeadContent,
    Outlet,
    Scripts,
} from "@tanstack/solid-router";
import { TanStackRouterDevtools } from "@tanstack/solid-router-devtools";
import { Suspense } from "solid-js";
import { HydrationScript } from "solid-js/web";

import Header from "../components/Header";

import styleCss from "../index.css?url";

export const Route = createRootRouteWithContext()({
    head: () => ({
        links: [{ rel: "stylesheet", href: styleCss }],
    }),
    shellComponent: RootComponent,
});

function RootComponent() {
    return (
        <html lang="en">
            <head>
                <HydrationScript />
            </head>
            <body>
                <HeadContent />
                <Suspense>
                    <Header />

                    <Outlet />
                    <TanStackRouterDevtools />
                </Suspense>
                <Scripts />
            </body>
        </html>
    );
}
