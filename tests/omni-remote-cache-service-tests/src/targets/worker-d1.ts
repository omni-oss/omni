import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { promisify } from "node:util";

import { sleep } from "../utils";
import { workerTargetFor } from "./worker";

const run = promisify(execFile);
const DB = "orcs-e2e";
const API_KEY_HASH = createHash("sha256").update("key1").digest("hex");

// D1 rejects a query that binds more parameters than it references, so
// every query references each parameter it is given.
const refs = (from: number, to: number) =>
    Array.from(
        { length: to - from + 1 },
        (_, i) => ` AND ?${from + i} IS NOT NULL`,
    ).join("");

const repositoryConfig = {
    queries: {
        tenant_exists: "SELECT 1 AS found FROM tenants WHERE code = ?1",
        organization_exists: "SELECT 1 AS found FROM orgs WHERE code = ?1",
        organization_belongs_to_tenant:
            "SELECT 1 AS found FROM orgs WHERE tenant = ?1 AND code = ?2",
        workspace_exists: "SELECT 1 AS found FROM workspaces WHERE code = ?1",
        workspace_belongs_to_organization:
            "SELECT 1 AS found FROM workspaces WHERE org = ?1 AND code = ?2",
        environment_exists: "SELECT 1 AS found FROM envs WHERE code = ?1",
        environment_belongs_to_workspace:
            "SELECT 1 AS found FROM envs WHERE ws = ?1 AND code = ?2",
        api_key_valid:
            "SELECT 1 AS found FROM api_keys WHERE key_hash = ?1 LIMIT 1",
        api_key_tenant:
            "SELECT 1 AS found FROM api_keys WHERE key_hash = ?1 AND tenant = ?2",
        api_key_organization: `SELECT (COUNT(*) > 0) AS found FROM api_keys WHERE key_hash = ?1${refs(2, 3)}`,
        api_key_workspace: `SELECT (COUNT(*) > 0) AS found FROM api_keys WHERE key_hash = ?1${refs(2, 4)}`,
        api_key_environment: `SELECT (COUNT(*) > 0) AS found FROM api_keys WHERE key_hash = ?1${refs(2, 5)}`,
        api_key_scope:
            "SELECT 1 AS found FROM key_scopes WHERE key_hash = ?1 AND scope = ?2",
    },
    hooks: {
        queries: {
            before: `SELECT reason AS veto_reason FROM vetoes WHERE tenant = ?2 AND (op = ?1 OR op = '*')${refs(3, 7)} LIMIT 1`,
            after: `INSERT INTO usage SELECT ?1, ?2, ?6, ?7, ?8 WHERE 1 = 1${refs(3, 5)}`,
        },
    },
};

const seed = `
CREATE TABLE tenants(code TEXT);
CREATE TABLE orgs(tenant TEXT, code TEXT);
CREATE TABLE workspaces(org TEXT, code TEXT);
CREATE TABLE envs(ws TEXT, code TEXT);
CREATE TABLE api_keys(key_hash TEXT, tenant TEXT);
CREATE TABLE key_scopes(key_hash TEXT, scope TEXT);
CREATE TABLE vetoes(tenant TEXT, op TEXT, reason TEXT);
CREATE TABLE usage(op TEXT, tenant TEXT, digest TEXT, outcome TEXT, api_key_id TEXT);
INSERT INTO tenants VALUES ('tenant1');
INSERT INTO orgs VALUES ('tenant1', 'org1');
INSERT INTO workspaces VALUES ('org1', 'ws1');
INSERT INTO envs VALUES ('ws1', 'env1');
INSERT INTO api_keys VALUES ('${API_KEY_HASH}', 'tenant1');
INSERT INTO key_scopes VALUES
  ('${API_KEY_HASH}', 'read:artifacts'), ('${API_KEY_HASH}', 'write:artifacts'),
  ('${API_KEY_HASH}', 'list:artifacts'), ('${API_KEY_HASH}', 'delete:artifacts');
`;

/**
 * The worker with ownership data, API keys and hooks read from a local D1
 * database through operator queries. Seeded with the same tenant, key and
 * scopes as `orcs.config.json`, so the shared specs run unchanged.
 */
export const workerD1Target = workerTargetFor({
    name: "worker-d1",
    basePort: 3600,
    async prepare({ workerDir, state, configFile }) {
        fs.appendFileSync(
            configFile,
            `\n[[d1_databases]]\nbinding = "DB"\ndatabase_name = "${DB}"\ndatabase_id = "00000000-0000-0000-0000-0000000000e2"\n`,
        );
        const seedFile = path.join(state, "seed.sql");
        fs.writeFileSync(seedFile, seed);

        // Concurrent `bunx wrangler@latest` installs can race on first use.
        const wrangler = async (args: string[]) => {
            let last: unknown;
            for (let i = 0; i < 3; i++) {
                try {
                    return await run(
                        "bunx",
                        [
                            "wrangler@latest",
                            ...args,
                            "-c",
                            configFile,
                            "--local",
                            "--persist-to",
                            state,
                        ],
                        { cwd: workerDir },
                    );
                } catch (e) {
                    last = e;
                    await sleep(2000);
                }
            }
            throw last;
        };
        await wrangler(["d1", "execute", DB, "--file", seedFile]);

        return {
            args: [
                "--var",
                `ORCS_REPOSITORY_CONFIG:${JSON.stringify(repositoryConfig)}`,
            ],
            async sql(statement) {
                const { stdout } = await wrangler([
                    "d1",
                    "execute",
                    DB,
                    "--command",
                    statement,
                    "--json",
                ]);
                return stdout;
            },
        };
    },
});
