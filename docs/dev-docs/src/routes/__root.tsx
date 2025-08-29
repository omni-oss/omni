/// <reference types="vite/client" />
import {
    createRootRoute,
    HeadContent,
    Outlet,
    Scripts,
} from "@tanstack/react-router";
import { TanstackProvider } from "fumadocs-core/framework/tanstack";
import { RootProvider } from "fumadocs-ui/provider/base";
import type * as React from "react";
import appCss from "@/styles/app.css?url";

export const Route = createRootRoute({
    head: () => ({
        meta: [
            {
                charSet: "utf-8",
            },
            {
                name: "viewport",
                content: "width=device-width, initial-scale=1",
            },
            {
                title: "Fumadocs on TanStack Start",
            },
        ],
        links: [{ rel: "stylesheet", href: appCss }],
    }),
    component: RootComponent,
});

function RootComponent() {
    return (
        <RootDocument>
            <Outlet />
        </RootDocument>
    );
}

function RootDocument({ children }: { children: React.ReactNode }) {
    return (
        // biome-ignore lint/a11y/useHtmlLang: false
        <html suppressHydrationWarning>
            <head>
                <HeadContent />
            </head>
            <body className="flex flex-col min-h-screen">
                <TanstackProvider>
                    <RootProvider>{children}</RootProvider>
                </TanstackProvider>
                <Scripts />
            </body>
        </html>
    );
}
