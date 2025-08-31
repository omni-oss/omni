"use client";
import { RootProvider } from "fumadocs-ui/provider/base";
import type { ReactNode } from "react";
// your custom dialog
import SearchDialog from "@/components/static-search";

export function AppProvider({ children }: { children: ReactNode }) {
    return (
        <RootProvider
            search={{
                SearchDialog,
            }}
        >
            {children}
        </RootProvider>
    );
}
