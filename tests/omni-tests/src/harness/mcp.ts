/**
 * MCP client harness for `omni mcp` e2e tests.
 *
 * Wraps {@link Client} + {@link StdioClientTransport} so tests can spawn an
 * `omni mcp` server backed by a temporary workspace, exercise tools via the
 * MCP protocol, and disconnect without boilerplate.
 */

import { Client, StdioClientTransport } from "@modelcontextprotocol/client";
import { onTestFinished } from "vitest";
import { resolveOmniBin } from "./binary";

export interface McpClient {
    /** The connected MCP client. Use to call tools and list capabilities. */
    readonly client: Client;
    /** Disconnect the client and terminate the server process. */
    disconnect(): Promise<void>;
}

export interface ConnectMcpOptions {
    /**
     * Workspace root directory. Passed as `--root-dir` to `omni mcp` so the
     * server locates `workspace.omni.yaml` regardless of where the test runs.
     */
    cwd: string;
}

/**
 * Spawn `omni mcp --root-dir <cwd>`, perform the MCP initialization handshake,
 * and return a connected {@link McpClient}.
 *
 * When called inside a Vitest test the client is automatically disconnected
 * after the test finishes. Outside a test call {@link McpClient.disconnect}.
 *
 * @example
 * const ws = makeWorkspace(singleProjectSpec());
 * const { client } = await connectMcp({ cwd: ws.cwd });
 * const result = await client.callTool({ name: "project_list" });
 */
export async function connectMcp(
    options: ConnectMcpOptions,
): Promise<McpClient> {
    const bin = resolveOmniBin();
    const transport = new StdioClientTransport({
        command: bin,
        args: ["mcp", "--root-dir", options.cwd],
        cwd: options.cwd,
        // Suppress server-side tracing from polluting test output.
        stderr: "ignore",
    });
    const client = new Client({ name: "omni-e2e-test", version: "0.0.1" });
    await client.connect(transport);

    const mcp: McpClient = {
        client,
        async disconnect() {
            try {
                await client.close();
            } catch {
                // Already closed or server already exited — harmless.
            }
        },
    };

    try {
        // onTestFinished throws outside a test context; ignore in that case.
        onTestFinished(() => void mcp.disconnect());
    } catch {
        // Not inside a test — caller is responsible for calling disconnect().
    }

    return mcp;
}
