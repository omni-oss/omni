import { Format } from "./format";
import type { Profile } from "./profile";

export const BUILT_IN_PROFILES: Profile[] = [
    {
        type: "path",
        files: ["package.json"],
        format: Format.JSON,
        path: ["version"],
    },
    {
        type: "regex",
        files: ["Cargo.toml"],
        pattern: '^\\s*version\\s*=\\s*"(?<version>.*)"\\s*$',
        flags: "m",
    },
    {
        type: "path",
        files: ["*.csproj"],
        path: ["Project", "PropertyGroup", "Version"],
        format: Format.XML,
    },
] as const;
