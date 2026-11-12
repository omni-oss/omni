import { createHash } from "node:crypto";

import { expect, test } from "vitest";

import { sleep, withTimeout } from "@/utils";

import { describeTargets } from "../test";

const DIGEST = "12345678901234567890123456789012345678901234567890123456789012";
const API_KEY = "key1";
const TENANT = "tenant1";
const ORG = "org1";
const WS = "ws1";
const ENV = "env1";
const BODY = JSON.stringify({
    artifact: {
        name: "test",
        type: "test",
    },
});

describeTargets("v1.artifacts.api", (target) => {
    test("put artifact", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const response = await putArtifact(apiBaseUrl);

        expect(response.status).toBeOneOf([200, 204]);
    });

    test("list artifacts", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        await putArtifact(apiBaseUrl);
        const response = await listArtifacts(apiBaseUrl);
        const body = await response.json();

        expect(response.status).toBe(200);
        expect(body.data).toHaveLength(1);
        expect(body.data[0].digest).toBe(DIGEST);
        expect(body.data[0].size).toBe(BODY.length);
    });

    test("get artifact", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        await putArtifact(apiBaseUrl);

        const response = await getArtifact(apiBaseUrl);

        expect(response.status).toBe(200);
        expect(response.headers.get("Content-Type")).toBe(
            "application/octet-stream",
        );
        expect(await response.text()).toEqual(BODY);
    });

    test("get artifact not found", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const response = await getArtifact(apiBaseUrl, crypto.randomUUID());

        expect(response.status).toBe(404);
    });

    test("artifact exists", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        await putArtifact(apiBaseUrl);

        const response = await headArtifact(apiBaseUrl);

        expect(response.status).toBeOneOf([200, 204]);
    });

    test("artifact exists not found", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const response = await headArtifact(apiBaseUrl, crypto.randomUUID());

        expect(response.status).toBe(404);
    });

    test("head artifacts", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        await putArtifact(apiBaseUrl);
        const response = await headArtifacts(apiBaseUrl);

        expect(response.status).toBeOneOf([200, 204]);
    });

    test("head artifacts invalid api key", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        await putArtifact(apiBaseUrl);
        const response = await headArtifacts(
            apiBaseUrl,
            TENANT,
            ORG,
            WS,
            ENV,
            "invalid",
        );

        expect(response.status).toBe(403);
    });

    test("large artifact round trip", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        // big enough to exercise chunked/multipart uploads in the backends
        const big = new Uint8Array(20 * 1024 * 1024);
        for (let i = 0; i < big.length; i += 65536) {
            crypto.getRandomValues(big.subarray(i, i + 65536));
        }
        const digest = crypto.randomUUID();

        const put = await putArtifact(
            apiBaseUrl,
            digest,
            TENANT,
            ORG,
            WS,
            ENV,
            big,
        );
        expect(put.status).toBe(204);

        const get = await fetch(
            `${apiBaseUrl}/v1/artifacts/${digest}?org=${ORG}&ws=${WS}&env=${ENV}`,
            { headers: { "X-OMNI-TENANT": TENANT, "X-API-KEY": API_KEY } },
        );
        expect(get.status).toBe(200);
        expect(Buffer.compare(Buffer.from(await get.arrayBuffer()), big)).toBe(
            0,
        );
    }, 30_000);

    test("delete artifact", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        await putArtifact(apiBaseUrl);

        const deleteResponse = await deleteArtifact(apiBaseUrl);
        const getResponse = await getArtifact(apiBaseUrl);

        expect(deleteResponse.status).toBe(204);
        expect(getResponse.status).toBe(404);
    });

    // Guards the D1 and `waitUntil` hook path, which only a database-backed
    // target has and which nothing else can run. Last, because it adds a veto.
    test("database hooks record usage after the response and can veto", async (ctx) => {
        if (!target.sql) ctx.skip();
        const sql = target.sql!;

        // A unique digest ties the assertion to this upload's after hook.
        const digest = crypto.randomUUID();
        const put = await putArtifact(target.apiBaseUrl, digest);
        expect(put.status).toBeOneOf([200, 204]);

        let usage = "";
        for (let i = 0; i < 50 && !usage.includes("stored"); i++) {
            usage = await sql(
                `SELECT outcome, api_key_id FROM usage WHERE digest = '${digest}' AND op = 'put'`,
            );
            await sleep(100);
        }
        expect(usage).toContain("stored");
        // attributed to an opaque id, never the key
        expect(usage).toContain(
            createHash("sha256").update(API_KEY).digest("hex").slice(0, 16),
        );

        await sql(`INSERT INTO vetoes VALUES ('${TENANT}', 'get', 'pruning')`);
        const vetoed = await getArtifact(target.apiBaseUrl);
        expect(vetoed.status).toBe(403);
        expect(await vetoed.text()).toContain("pruning");
    }, 60_000);
});

// Usage with fetch
const fetchWithTimeout = withTimeout(fetch, 10_000);

async function putArtifact(
    apiBaseUrl: string,
    digest: string = DIGEST,
    tenant: string = TENANT,
    org: string = ORG,
    ws: string = WS,
    env: string = ENV,
    body: string | Uint8Array<ArrayBuffer> = BODY,
    apiKey: string = API_KEY,
) {
    return await fetchWithTimeout(
        `${apiBaseUrl}/v1/artifacts/${digest}?org=${org}&ws=${ws}&env=${env}`,
        {
            method: "PUT",
            headers: {
                "Content-Type": "application/octet-stream",
                "X-OMNI-TENANT": tenant,
                "X-API-KEY": apiKey,
            },
            body,
        },
    );
}

async function getArtifact(
    apiBaseUrl: string,
    digest: string = DIGEST,
    tenant: string = TENANT,
    org: string = ORG,
    ws: string = WS,
    env: string = ENV,
    apiKey: string = API_KEY,
) {
    return await fetchWithTimeout(
        `${apiBaseUrl}/v1/artifacts/${digest}?org=${org}&ws=${ws}&env=${env}`,
        {
            method: "GET",
            headers: {
                "Content-Type": "application/octet-stream",
                "X-OMNI-TENANT": tenant,
                "X-API-KEY": apiKey,
            },
        },
    );
}

async function headArtifact(
    apiBaseUrl: string,
    digest: string = DIGEST,
    tenant: string = TENANT,
    org: string = ORG,
    ws: string = WS,
    env: string = ENV,
    apiKey: string = API_KEY,
) {
    return await fetchWithTimeout(
        `${apiBaseUrl}/v1/artifacts/${digest}?org=${org}&ws=${ws}&env=${env}`,
        {
            method: "HEAD",
            headers: {
                "Content-Type": "application/octet-stream",
                "X-OMNI-TENANT": tenant,
                "X-API-KEY": apiKey,
            },
        },
    );
}

async function deleteArtifact(
    apiBaseUrl: string,
    digest: string = DIGEST,
    tenant: string = TENANT,
    org: string = ORG,
    ws: string = WS,
    env: string = ENV,
    apiKey: string = API_KEY,
) {
    return await fetchWithTimeout(
        `${apiBaseUrl}/v1/artifacts/${digest}?org=${org}&ws=${ws}&env=${env}`,
        {
            method: "DELETE",
            headers: {
                "Content-Type": "application/octet-stream",
                "X-OMNI-TENANT": tenant,
                "X-API-KEY": apiKey,
            },
        },
    );
}

async function listArtifacts(
    apiBaseUrl: string,
    tenant: string = TENANT,
    org: string = ORG,
    ws: string = WS,
    env: string = ENV,
    apiKey: string = API_KEY,
) {
    return await fetchWithTimeout(
        `${apiBaseUrl}/v1/artifacts?org=${org}&ws=${ws}&env=${env}`,
        {
            method: "GET",
            headers: {
                "X-OMNI-TENANT": tenant,
                "X-API-KEY": apiKey,
            },
        },
    );
}

async function headArtifacts(
    apiBaseUrl: string,
    tenant: string = TENANT,
    org: string = ORG,
    ws: string = WS,
    env: string = ENV,
    apiKey: string = API_KEY,
) {
    return await fetchWithTimeout(
        `${apiBaseUrl}/v1/artifacts?org=${org}&ws=${ws}&env=${env}`,
        {
            method: "HEAD",
            headers: {
                "X-OMNI-TENANT": tenant,
                "X-API-KEY": apiKey,
            },
        },
    );
}
