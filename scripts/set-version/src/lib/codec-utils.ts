import path from "node:path";
import JSONC from "comment-json";
import { XMLBuilder, XMLParser } from "fast-xml-parser";
import TOML from "smol-toml";
import YAML from "yaml";
import { Format } from "./format";

const xmlOptions = {
    preserveOrder: true, // Required to keep comments near their tags
    commentPropName: "#comment", // Captures comments into this key
    ignoreAttributes: false,
};

const XML = {
    __$$parser: new XMLParser(xmlOptions),
    __$$builder: new XMLBuilder(xmlOptions),
    parse: (content: string) => XML.__$$parser.parse(content) as unknown,
    stringify: (object: unknown) => XML.__$$builder.build(object),
};

export function deserialize(
    filePath: string,
    content: string,
    format: Format = Format.AUTO,
) {
    if (format === Format.AUTO) {
        format = autoDetectFormat(filePath);
    }
    switch (format) {
        case Format.JSON:
            return JSONC.parse(content);
        case Format.YAML:
            return YAML.parse(content);
        case Format.XML:
            return XML.parse(content);
        case Format.TOML:
            return TOML.parse(content);
        default:
            throw new UnsupportedFileTypeError(filePath);
    }
}

export function serialize(
    filePath: string,
    object: unknown,
    format: Format = Format.AUTO,
): string {
    if (format === Format.AUTO) {
        format = autoDetectFormat(filePath);
    }
    switch (format) {
        case Format.JSON:
            return JSONC.stringify(object, null, 4);
        case Format.YAML:
            return YAML.stringify(object);
        case Format.XML:
            return XML.stringify(object);
        case Format.TOML:
            return TOML.stringify(object);
        default:
            throw new UnsupportedFileTypeError(filePath);
    }
}

export function autoDetectFormat(filePath: string): Format {
    const ext = path.extname(filePath);
    switch (ext) {
        case ".json":
        case ".jsonc":
            return Format.JSON;
        case ".yaml":
        case ".yml":
            return Format.YAML;
        case ".xml":
            return Format.XML;
        case ".toml":
            return Format.TOML;
        default:
            throw new UnsupportedFileTypeError(filePath);
    }
}

export class UnsupportedFileTypeError extends Error {
    constructor(file: string) {
        super(`Unsupported file type for file ${file}`);
        super.name = this.constructor.name;
    }
}
