import path from "node:path";
import type { System } from "@omni-oss/system-interface";
import picomatch from "picomatch";
import YAML from "yaml";
import { autoDetectFormat, deserialize, serialize } from "./codec-utils";
import { Format } from "./format";
import type { PathProfile, Profile } from "./profile";
import { NoValueAtPathError, setValueIn } from "./set-value";
import { RegexNotMatchedError, replaceGroup } from "./string-utils";

export type SetVersionOptions = {
    dryRun?: boolean;
};

export type Matched = {
    path: string;
    changed: boolean;
    notChangedReason?: NotChangedReason | undefined;
    notChangedReasonMessage?: string | undefined;
};

export enum NotChangedReason {
    NO_VALUE_AT_PATH = "NO_VALUE_AT_PATH",
    REGEX_PATTERN_NOT_MATCHED = "REGEX_PATTERN_NOT_MATCHED",
    ALREADY_UP_TO_DATE = "ALREADY_UP_TO_DATE",
}

export async function setVersionAtDir(
    dir: string,
    version: string,
    profiles: Profile[],
    system: System,
    options: SetVersionOptions = {},
): Promise<Matched[]> {
    const allGlobs = Array.from(profiles.flatMap((p) => p.files));
    const mainGlob = picomatch(allGlobs);
    const profileWithGlob = profiles.map((p) => ({
        ...p,
        glob: picomatch(p.files),
    }));

    const files = await system.fs.readDirectory(dir);
    const matched: Matched[] = [];
    for (const matchedFile of files) {
        if (!mainGlob(matchedFile) || !(await system.fs.isFile(matchedFile))) {
            continue;
        }
        const fullPath = path.join(dir, matchedFile);

        const original = await system.fs.readFileAsString(fullPath);
        let fileContent = original;
        const profiles = profileWithGlob.filter((p) => p.glob(matchedFile));
        if (!profiles.length) {
            throw new NoProfileFoundError(matchedFile);
        }
        let notChangedReason: NotChangedReason | undefined;
        let notChangedReasonMessage: string | undefined;
        for (const profile of profiles) {
            try {
                fileContent = applyVersion(
                    { path: fullPath, content: fileContent },
                    version,
                    profile,
                );
            } catch (e) {
                if (e instanceof NoValueAtPathError) {
                    notChangedReason = NotChangedReason.NO_VALUE_AT_PATH;
                    notChangedReasonMessage = `No value at path ${e.path.join(
                        ".",
                    )}, make sure the path is correct and it exists`;
                } else if (e instanceof RegexNotMatchedError) {
                    notChangedReason =
                        NotChangedReason.REGEX_PATTERN_NOT_MATCHED;
                    notChangedReasonMessage = `Regex pattern ${e.pattern} did not match`;
                } else {
                    throw e;
                }
            }
        }

        const changed = original !== fileContent;

        matched.push({
            path: fullPath,
            changed,
            notChangedReason: changed
                ? undefined
                : (notChangedReason ?? NotChangedReason.ALREADY_UP_TO_DATE),
            notChangedReasonMessage: changed
                ? undefined
                : (notChangedReasonMessage ?? "File already up to date"),
        });

        if (!options.dryRun && original !== fileContent) {
            await system.fs.writeStringToFile(fullPath, fileContent);
        }
    }
    return matched;
}

export class NoProfileFoundError extends Error {
    constructor(file: string) {
        super(`No profile found for file ${file}`);
        super.name = this.constructor.name;
    }
}

type File = {
    path: string;
    content: string;
};

export function applyVersion(
    file: File,
    version: string,
    profile: Profile,
): string {
    switch (profile.type) {
        case "path": {
            const fmt =
                (profile.format === Format.AUTO
                    ? autoDetectFormat(file.path)
                    : profile.format) ?? Format.AUTO;

            if (profile.format === Format.YAML) {
                const document = YAML.parseDocument(file.content);
                document.setIn(profile.path, version);
                return document.toString();
            } else if (profile.format === Format.XML) {
                return applyVersionPathXmlStrategy(file, version, profile, fmt);
            } else {
                return applyVersionPathGenericStrategy(
                    file,
                    version,
                    profile,
                    fmt,
                );
            }
        }
        case "regex": {
            const regex = new RegExp(profile.pattern, profile.flags ?? "m");
            return replaceGroup(
                file.content,
                regex,
                profile.capture_group ?? "version",
                version,
            );
        }
        default:
            // biome-ignore lint/suspicious/noExplicitAny: escape typesafety for this
            throw new UnsupportedProfileTypeError((profile as any).type);
    }
}

function applyVersionPathGenericStrategy(
    file: File,
    version: string,
    profile: PathProfile,
    format: Format,
) {
    const parsed = deserialize(file.path, file.content, format);

    // biome-ignore lint/suspicious/noExplicitAny: escape typesafety for this
    const newValue = setValueIn(parsed, profile.path as any, version);
    const serialized = serialize(file.path, newValue, format);

    return serialized;
}

function applyVersionPathXmlStrategy(
    file: File,
    version: string,
    profile: PathProfile,
    format: Format,
) {
    const parsed = deserialize(file.path, file.content, format);

    // biome-ignore lint/suspicious/noExplicitAny: escape typesafety for this
    const path = convertPathToXmlPath(profile.path as any, parsed) as any;
    const newValue = setValueIn(parsed, path, version);
    const serialized = serialize(file.path, newValue, format);

    return serialized;
}

// biome-ignore lint/suspicious/noExplicitAny:  escape typesafety for this
function convertPathToXmlPath(paths: (number | string)[], object: any[]) {
    const xmlPath: (string | number)[] = [];
    let currentObject = object;
    for (const path of paths) {
        const entries = Array.from(currentObject.entries());
        for (const [jIndex, entry] of entries) {
            if (entry[path]) {
                xmlPath.push(jIndex, path);
                currentObject = entry[path];
                break;
            }

            if (entries.length === jIndex + 1) {
                throw new NoValueAtPathError(paths);
            }
        }
    }

    const entries = Array.from(currentObject.entries());
    for (const [index, entry] of entries) {
        if (entry["#text"]) {
            xmlPath.push(index, "#text");
            break;
        }

        if (entries.length === index + 1) {
            throw new NoValueAtPathError(paths);
        }
    }
    return xmlPath;
}

export class UnsupportedProfileTypeError extends Error {
    constructor(type: string) {
        super(`Unsupported profile type ${type}`);
        super.name = this.constructor.name;
    }
}
