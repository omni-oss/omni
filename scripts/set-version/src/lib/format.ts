import z from "zod";

export enum Format {
    AUTO = "auto",
    YAML = "yaml",
    JSON = "json",
    TOML = "toml",
    XML = "xml",
}

export const FormatSchema = z.enum(Format);
