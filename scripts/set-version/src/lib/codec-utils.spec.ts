import { describe, expect, it } from "vitest";
import { autoDetectFormat, deserialize, serialize } from "./codec-utils";
import { Format } from "./format";

const deserialized = { name: "test" };

type SerdeTestData = {
    file: string;
    serialized: string;
    format: Format;
    deserialized: unknown;
};

const SERDE_TEST_DATA: SerdeTestData[] = [
    {
        file: "package.json",
        serialized: JSON.stringify(deserialized, null, 4),
        format: Format.JSON,
        deserialized,
    },
    {
        file: "package.jsonc",
        serialized: JSON.stringify(deserialized, null, 4),
        format: Format.JSON,
        deserialized,
    },
    {
        file: "package.yaml",
        serialized: "name: test\n",
        format: Format.YAML,
        deserialized,
    },
    {
        file: "package.yml",
        serialized: "name: test\n",
        format: Format.YAML,
        deserialized,
    },
    {
        file: "package.xml",
        serialized: "<name>test</name>",
        format: Format.XML,
        deserialized: [{ name: [{ "#text": "test" }] }],
    },
    {
        file: "package.toml",
        serialized: 'name = "test"\n',
        format: Format.TOML,
        deserialized,
    },
];

describe("autoDetectFormat", () => {
    it.each(SERDE_TEST_DATA)("should detect format ($file)", async ({
        file,
        format,
    }) => {
        expect(autoDetectFormat(file)).toEqual(format);
    });
});

describe("serialize", () => {
    it.each(SERDE_TEST_DATA)("should serialize $format", ({
        file,
        serialized,
        format,
        deserialized,
    }) => {
        expect(serialize(file, deserialized, format)).toEqual(serialized);
    });
});

describe("deserialize", () => {
    it.each(SERDE_TEST_DATA)("should deserialize $format", ({
        file,
        serialized,
        format,
        deserialized,
    }) => {
        expect(deserialize(file, serialized, format)).toEqual(deserialized);
    });
});
