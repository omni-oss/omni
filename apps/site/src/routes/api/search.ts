import { createServerFileRoute } from "@tanstack/react-start/server";
import { searchServer } from "@/lib/search-server.shared";

export const ServerRoute = createServerFileRoute("/api/search").methods({
    GET: async ({ request: _ }) => searchServer.staticGET(),
});
