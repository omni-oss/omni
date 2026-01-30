import { VirtualSystem } from "@omni-oss/system-interface";
import JSONC from "comment-json";
import { XMLBuilder, XMLParser } from "fast-xml-parser";
import TOML from "smol-toml";
import { describe, expect, it } from "vitest";
import YAML from "yaml";
import { UnsupportedFileTypeError } from "./codec-utils";
import { Format } from "./format";
import type { TaggedPathProfile, TaggedRegexProfile } from "./profile";
import { applyVersion, setVersionAtDir } from "./set-version";

const DATA = {
    name: "test",
    version: "0.0.0",
};

const NEW_VERSION = "1.0.0";

const PATH_PROFILES = [
    {
        type: "path",
        format: Format.JSON,
        path: ["version"],
        files: ["package.json"] as const,
    },
    {
        type: "path",
        format: Format.YAML,
        path: ["version"],
        files: ["package.yaml"] as const,
    },
    {
        type: "path",
        format: Format.XML,
        path: ["version"],
        files: ["package.xml"] as const,
    },
    {
        type: "path",
        format: Format.TOML,
        path: ["version"],
        files: ["package.toml"] as const,
    },
] satisfies TaggedPathProfile[];

describe("applyVersion", () => {
    it.each(
        PATH_PROFILES,
    )("apply version using path profile ($format)", (profile) => {
        const fileName = profile.files[0];
        const file = serialize(fileName, profile.format, DATA);

        const updatedContent = applyVersion(
            {
                content: file,
                path: fileName,
            },
            NEW_VERSION,
            profile,
        );

        const updated = deserialize(fileName, profile.format, updatedContent);

        expect(updated).toEqual({
            ...DATA,
            version: NEW_VERSION,
        });
    });

    it("apply version using regex profile", () => {
        const regexProfile: TaggedRegexProfile = {
            type: "regex",
            files: ["package.json"],
            pattern: '\\s*"version"\\s*:\\s*"(?<version>.*)"\\s*',
        };
        const updatedContent = applyVersion(
            {
                content: JSONC.stringify(DATA),
                path: "package.json",
            },
            NEW_VERSION,
            regexProfile,
        );
        const updated = JSONC.parse(updatedContent);

        expect(updated).toEqual({
            ...DATA,
            version: NEW_VERSION,
        });
    });
});

describe("setVersionAtDir", () => {
    it.each(
        PATH_PROFILES,
    )("should set version of files in a directory ($format)", async (profile) => {
        const system = await VirtualSystem.create();
        await system.fs.createDirectory("/test");
        system.proc.setCurrentDir("/test");

        const fileName = profile.files[0];
        await system.fs.writeStringToFile(
            fileName,
            serialize(fileName, profile.format, DATA),
        );

        await setVersionAtDir(
            system.proc.currentDir(),
            NEW_VERSION,
            [profile],
            system,
        );

        const updated = deserialize(
            fileName,
            profile.format,
            await system.fs.readFileAsString(fileName),
        );

        expect(updated).toEqual({
            ...DATA,
            version: NEW_VERSION,
        });
    });

    it("should support globs", async () => {
        const system = await VirtualSystem.create();
        await system.fs.createDirectory("/test");
        system.proc.setCurrentDir("/test");

        const profile = {
            type: "path",
            files: ["*.csproj"],
            format: Format.XML,
            path: ["version"],
        } satisfies TaggedPathProfile;

        const fileName = "TestProject.csproj";

        await system.fs.writeStringToFile(
            fileName,
            serialize(fileName, profile.format, DATA),
        );

        await setVersionAtDir(
            system.proc.currentDir(),
            NEW_VERSION,
            [profile],
            system,
        );

        const updated = deserialize(
            fileName,
            profile.format,
            await system.fs.readFileAsString(fileName),
        );

        expect(updated).toEqual({
            ...DATA,
            version: NEW_VERSION,
        });
    });

    it("should support regex", async () => {
        const system = await VirtualSystem.create();
        await system.fs.createDirectory("/test");
        system.proc.setCurrentDir("/test");

        const profile = {
            type: "regex",
            files: ["*.csproj"],
            pattern: "<version>(?<version>.*)</version>",
        } satisfies TaggedRegexProfile;

        const fileName = "TestProject.csproj";

        await system.fs.writeStringToFile(
            fileName,
            serialize(fileName, Format.XML, DATA),
        );

        await setVersionAtDir(
            system.proc.currentDir(),
            NEW_VERSION,
            [profile],
            system,
        );

        const updated = deserialize(
            fileName,
            Format.XML,
            await system.fs.readFileAsString(fileName),
        );

        expect(updated).toEqual({
            ...DATA,
            version: NEW_VERSION,
        });
    });

    it("should not write changes to disk if dryRun is true", async () => {
        const system = await VirtualSystem.create();
        await system.fs.createDirectory("/test");
        system.proc.setCurrentDir("/test");

        const profile = {
            type: "path",
            files: ["*.csproj"],
            format: Format.XML,
            path: ["version"],
        } satisfies TaggedPathProfile;

        const fileName = "TestProject.csproj";

        await system.fs.writeStringToFile(
            fileName,
            serialize(fileName, profile.format, DATA),
        );

        await setVersionAtDir(
            system.proc.currentDir(),
            NEW_VERSION,
            [profile],
            system,
            { dryRun: true },
        );

        const updated = deserialize(
            fileName,
            profile.format,
            await system.fs.readFileAsString(fileName),
        );

        expect(updated).toEqual({
            ...DATA,
            version: DATA.version,
        });
    });
});

function serialize(fileName: string, format: Format, object: unknown): string {
    switch (format) {
        case Format.JSON:
            return JSONC.stringify(object, null, 4);
        case Format.YAML:
            return YAML.stringify(object);
        case Format.XML:
            return new XMLBuilder().build(object);
        case Format.TOML:
            return TOML.stringify(object);
        default:
            throw new UnsupportedFileTypeError(fileName);
    }
}

function deserialize(
    filaName: string,
    format: Format,
    content: string,
): unknown {
    switch (format) {
        case Format.JSON:
            return JSONC.parse(content);
        case Format.YAML:
            return YAML.parse(content);
        case Format.XML:
            return new XMLParser().parse(content);
        case Format.TOML:
            return TOML.parse(content);
        default:
            throw new UnsupportedFileTypeError(filaName);
    }
}
