import { describe, expect } from "vitest";
import { test } from "../test";

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

describe("v1.artifacts.api", () => {
    test("put artifact", async ({ apiBaseUrl }) => {
        const response = await putArtifact(apiBaseUrl);

        expect(response.status).toBeOneOf([200, 204]);
    });

    test("list artifacts", async ({ apiBaseUrl }) => {
        await putArtifact(apiBaseUrl);
        const response = await listArtifacts(apiBaseUrl);
        const body = await response.json();

        expect(response.status).toBe(200);
        expect(body.data).toHaveLength(1);
        expect(body.data[0].digest).toBe(DIGEST);
        expect(body.data[0].size).toBe(BODY.length);
    });

    test("get artifact", async ({ apiBaseUrl }) => {
        await putArtifact(apiBaseUrl);

        const response = await getArtifact(apiBaseUrl);

        expect(response.status).toBe(200);
        expect(response.headers.get("Content-Type")).toBe(
            "application/octet-stream",
        );
        expect(await response.text()).toEqual(BODY);
    });

    test("get artifact not found", async ({ apiBaseUrl }) => {
        const response = await getArtifact(apiBaseUrl);

        expect(response.status).toBe(404);
    });

    test("artifact exists", async ({ apiBaseUrl }) => {
        await putArtifact(apiBaseUrl);

        const response = await headArtifact(apiBaseUrl);

        expect(response.status).toBeOneOf([200, 204]);
    });

    test("artifact exists not found", async ({ apiBaseUrl }) => {
        const response = await headArtifact(apiBaseUrl);

        expect(response.status).toBe(404);
    });

    test("head artifacts", async ({ apiBaseUrl }) => {
        await putArtifact(apiBaseUrl);
        const response = await headArtifacts(apiBaseUrl);

        expect(response.status).toBeOneOf([200, 204]);
    });

    test("head artifacts invalid api key", async ({ apiBaseUrl }) => {
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

    test("delete artifact", async ({ apiBaseUrl }) => {
        await putArtifact(apiBaseUrl);

        const deleteResponse = await deleteArtifact(apiBaseUrl);
        const getResponse = await getArtifact(apiBaseUrl);

        expect(deleteResponse.status).toBe(204);
        expect(getResponse.status).toBe(404);
    });
});

async function putArtifact(
    apiBaseUrl: string,
    digest: string = DIGEST,
    tenant: string = TENANT,
    org: string = ORG,
    ws: string = WS,
    env: string = ENV,
    body: string = BODY,
    apiKey: string = API_KEY,
) {
    return await fetch(
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
    return await fetch(
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
    return await fetch(
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
    return await fetch(
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
    return await fetch(
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
    return await fetch(
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
