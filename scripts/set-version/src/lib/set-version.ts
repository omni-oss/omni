import path from "node:path";
import type { System } from "@omni-oss/system-interface";
import picomatch from "picomatch";
import YAML from "yaml";
import { autoDetectFormat, deserialize, serialize } from "./codec-utils";
import { Format } from "./format";
import type { PathProfile, Profile } from "./profile";
import { setValueIn } from "./set-value";
import { replaceGroup } from "./string-utils";

export type SetVersionOptions = {
    dryRun?: boolean;
};

export async function setVersionAtDir(
    dir: string,
    version: string,
    profiles: Profile[],
    system: System,
    options: SetVersionOptions = {},
): Promise<string[]> {
    const allGlobs = Array.from(profiles.flatMap((p) => p.files));
    const mainGlob = picomatch(allGlobs);
    const profileWithGlob = profiles.map((p) => ({
        ...p,
        glob: picomatch(p.files),
    }));

    const files = await system.fs.readDirectory(dir);
    const updatedFiles: string[] = [];
    for (const matchedFile of files) {
        if (!mainGlob(matchedFile) || !(await system.fs.isFile(matchedFile))) {
            continue;
        }
        const fullPath = path.join(dir, matchedFile);
        updatedFiles.push(fullPath);

        let fileContent = await system.fs.readFileAsString(fullPath);
        const profiles = profileWithGlob.filter((p) => p.glob(matchedFile));
        if (!profiles.length) {
            throw new NoProfileFoundError(matchedFile);
        }
        for (const profile of profiles) {
            fileContent = applyVersion(
                { path: fullPath, content: fileContent },
                version,
                profile,
            );
        }

        if (!options.dryRun) {
            await system.fs.writeStringToFile(fullPath, fileContent);
        }
    }
    return updatedFiles;
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
        for (const [jIndex, entry] of currentObject.entries()) {
            if (entry[path]) {
                xmlPath.push(jIndex, path);
                currentObject = entry[path];
                break;
            }
        }
    }

    for (const [index, entry] of currentObject.entries()) {
        if (entry["#text"]) {
            xmlPath.push(index, "#text");
            break;
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
