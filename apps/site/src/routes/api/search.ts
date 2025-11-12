import { createFileRoute } from "@tanstack/react-router";
import { searchServer } from "@/lib/search-server.shared";

export const Route = createFileRoute("/api/search")({
    server: {
        handlers: {
            GET: async () => searchServer.staticGET(),
        },
    },
});
