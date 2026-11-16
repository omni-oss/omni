import { createHash, randomInt } from "node:crypto";

import { expect, test } from "vitest";

import { sleep, withTimeout } from "@/utils";

import { describeTargets } from "../test";

const BASE58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const DIGEST = "5RkZ7vQ2mXwYpLt3hNcFbA9dGjEsKuVoWiHaBxCqTyMn";
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

function randomDigest(): string {
    let digest = "";
    for (let i = 0; i < 44; i++) {
        digest += BASE58[randomInt(BASE58.length)];
    }
    return digest;
}

function sha256Hex(body: string | Uint8Array): string {
    return createHash("sha256").update(body).digest("hex");
}

describeTargets("v1.artifacts.api", (target) => {
    test("put artifact", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        // A fresh digest: entries are immutable, so a shared one would answer
        // 409 for every test after the first.
        const response = await putArtifact(apiBaseUrl, randomDigest());

        expect(response.status).toBeOneOf([200, 204]);
    });

    test("an existing artifact is immutable", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const digest = randomDigest();

        const first = await putArtifact(
            apiBaseUrl,
            digest,
            TENANT,
            ORG,
            WS,
            ENV,
            "first body",
        );
        expect(first.status).toBeOneOf([200, 204]);

        const second = await putArtifact(
            apiBaseUrl,
            digest,
            TENANT,
            ORG,
            WS,
            ENV,
            "second body",
        );
        expect(second.status).toBe(409);

        const get = await getArtifact(apiBaseUrl, digest);
        expect(get.status).toBe(200);
        expect(await get.text()).toBe("first body");
    });

    test("list artifacts", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        await putArtifact(apiBaseUrl);
        const response = await listArtifacts(apiBaseUrl);
        const body = await response.json();

        expect(response.status).toBe(200);
        // Entries are immutable and the container is shared across tests, so
        // other tests' digests may also be listed.
        const entry = body.data.find(
            (item: { digest: string }) => item.digest === DIGEST,
        );
        expect(entry).toBeDefined();
        // The stored object carries a provenance trailer, so it is larger than
        // the uploaded artifact.
        expect(entry.size).toBeGreaterThan(BODY.length);
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
        const response = await getArtifact(apiBaseUrl, randomDigest());

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
        const response = await headArtifact(apiBaseUrl, randomDigest());

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

        expect(response.status).toBe(401);
    });

    test("large artifact round trip", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        // big enough to exercise chunked/multipart uploads in the backends
        const big = new Uint8Array(20 * 1024 * 1024);
        for (let i = 0; i < big.length; i += 65536) {
            crypto.getRandomValues(big.subarray(i, i + 65536));
        }
        const digest = randomDigest();

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
            {
                headers: {
                    "X-OMNI-TENANT": TENANT,
                    Authorization: `Bearer ${API_KEY}`,
                },
            },
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

    test("invalid digest is rejected", async () => {
        const apiBaseUrl = target.apiBaseUrl;

        for (const digest of ["invalid-digest", "has.dot", "0abc", "lll"]) {
            expect((await getArtifact(apiBaseUrl, digest)).status).toBe(400);
            expect((await headArtifact(apiBaseUrl, digest)).status).toBe(400);
            expect((await putArtifact(apiBaseUrl, digest)).status).toBe(400);
            expect((await deleteArtifact(apiBaseUrl, digest)).status).toBe(400);
        }
    });

    test("put without content sha256 header is rejected", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const response = await fetchWithTimeout(
            `${apiBaseUrl}/v1/artifacts/${randomDigest()}?org=${ORG}&ws=${WS}&env=${ENV}`,
            {
                method: "PUT",
                headers: {
                    "Content-Type": "application/octet-stream",
                    "X-OMNI-TENANT": TENANT,
                    Authorization: `Bearer ${API_KEY}`,
                },
                body: BODY,
            },
        );

        expect(response.status).toBe(400);
    });

    test("put with a mismatched content sha256 is rejected and stores nothing", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const digest = randomDigest();

        const wrong = await putArtifact(
            apiBaseUrl,
            digest,
            TENANT,
            ORG,
            WS,
            ENV,
            BODY,
            API_KEY,
            "0".repeat(64),
        );
        expect(wrong.status).toBe(422);

        const get = await getArtifact(apiBaseUrl, digest);
        expect(get.status).toBe(404);

        const retry = await putArtifact(apiBaseUrl, digest);
        expect(retry.status).toBeOneOf([200, 204]);
    });

    test("put with uppercase content sha256 is rejected", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const response = await putArtifact(
            apiBaseUrl,
            randomDigest(),
            TENANT,
            ORG,
            WS,
            ENV,
            BODY,
            API_KEY,
            sha256Hex(BODY).toUpperCase(),
        );

        expect(response.status).toBe(400);
    });

    test("get returns the uploaded bytes and the stored metadata", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const digest = randomDigest();
        const hash = sha256Hex(BODY);
        await putArtifact(apiBaseUrl, digest);

        const response = await getArtifact(apiBaseUrl, digest);

        expect(response.status).toBe(200);
        expect(await response.text()).toEqual(BODY);
        expect(response.headers.get("x-omni-content-sha256")).toBe(hash);
        expect(response.headers.get("x-omni-stored-at")).toBeTruthy();
        expect(response.headers.get("x-omni-namespace")).toBe("shared");
    });

    test("head agrees with get about which entries exist", async () => {
        const apiBaseUrl = target.apiBaseUrl;
        const digest = randomDigest();
        await putArtifact(apiBaseUrl, digest);

        // HEAD answers 204 for an entry that GET serves with 200.
        expect((await getArtifact(apiBaseUrl, digest)).status).toBe(200);
        expect((await headArtifact(apiBaseUrl, digest)).status).toBeOneOf([
            200, 204,
        ]);
        expect((await headArtifact(apiBaseUrl, randomDigest())).status).toBe(
            404,
        );
    });

    // Guards the D1 and `waitUntil` hook path, which only a database-backed
    // target has and which nothing else can run. Last, because it adds a veto.
    test("database hooks record usage after the response and can veto", async (ctx) => {
        if (!target.sql) ctx.skip();
        const sql = target.sql!;

        // A unique digest ties the assertion to this upload's after hook.
        const digest = randomDigest();

        const put = await putArtifact(target.apiBaseUrl, digest);
        expect(put.status).toBeOneOf([200, 204]);

        let usage = "";
        for (let i = 0; i < 50 && !usage.includes("stored"); i++) {
            usage = await sql(
                `SELECT outcome, api_key_id, credential_kind, principal, namespace, sha256 FROM usage WHERE digest = '${digest}' AND op = 'put'`,
            );
            await sleep(100);
        }
        expect(usage).toContain("stored");
        // attributed to an opaque id, never the key
        const callerId = createHash("sha256")
            .update(API_KEY)
            .digest("hex")
            .slice(0, 16);
        expect(usage).toContain(callerId);
        expect(usage).not.toContain(API_KEY);
        // the same facts the stored entry records
        expect(usage).toContain("api_key");
        expect(usage).toContain("shared");
        expect(usage).toContain(sha256Hex(BODY));

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
    contentSha256: string = sha256Hex(body),
) {
    return await fetchWithTimeout(
        `${apiBaseUrl}/v1/artifacts/${digest}?org=${org}&ws=${ws}&env=${env}`,
        {
            method: "PUT",
            headers: {
                "Content-Type": "application/octet-stream",
                "X-OMNI-TENANT": tenant,
                Authorization: `Bearer ${apiKey}`,
                "X-Omni-Content-Sha256": contentSha256,
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
                Authorization: `Bearer ${apiKey}`,
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
                Authorization: `Bearer ${apiKey}`,
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
                Authorization: `Bearer ${apiKey}`,
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
                Authorization: `Bearer ${apiKey}`,
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
                Authorization: `Bearer ${apiKey}`,
            },
        },
    );
}
