export { BUILT_IN_PROFILES } from "./built-in-profiles";
export type { SetVersionConfig } from "./config";
export type { SetVersionOptions } from "./set-version";

import { OptimizedSystem } from "@omni-oss/system-interface";
import type { SetVersionConfig } from "./config";
import { findConfigAtDir } from "./find-config";
import type { Profile } from "./profile";
import { type SetVersionOptions, setVersionAtDir } from "./set-version";

export async function setVersion(
    dir: string,
    version: string,
    profiles: Profile[],
    options: SetVersionOptions = {},
) {
    return setVersionAtDir(
        dir,
        version,
        profiles,
        await OptimizedSystem.create(),
        options,
    );
}

export async function findConfig<TRequired extends boolean>(
    dir: string,
    required: TRequired,
): Promise<
    TRequired extends true ? SetVersionConfig : SetVersionConfig | undefined
> {
    return findConfigAtDir(dir, required, await OptimizedSystem.create());
}
