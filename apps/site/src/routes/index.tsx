import { createFileRoute } from "@tanstack/react-router";
import { HomeLayout } from "fumadocs-ui/layouts/home";
import { baseOptions } from "@/lib/layout.shared";

export const Route = createFileRoute("/")({
    component: Home,
});

function Home() {
    return (
        <HomeLayout
            {...baseOptions()}
            links={[
                {
                    type: "main",
                    on: "all",
                    text: "Documentation",
                    url: "/docs/quick-start",
                },
            ]}
            className="text-center py-32 justify-center"
        >
            <h1 className="font-medium text-xl mb-4">Omni Documentation.</h1>
        </HomeLayout>
    );
}
