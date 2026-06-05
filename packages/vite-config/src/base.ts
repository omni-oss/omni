import { builtinModules } from "node:module";
import { esmExternalRequirePlugin } from "rolldown/plugins";
import { mergeConfig, type Rolldown, type UserConfig } from "vite";
import type { PackageJson } from "./types";

const config: UserConfig = {
    plugins: [],
    resolve: {
        tsconfigPaths: true,
    },
};

export default config;

export type ExternalizeOption =
    | DependencyOption
    | ExternalizeDependencyTypesOption;

type DependencyOption = boolean | string[] | ((id: string) => boolean);

export type ExternalizeDependencyTypesOption = {
    dependencies?: DependencyOption;
    devDependencies?: DependencyOption;
    peerDependencies?: DependencyOption;
    nodeBuiltIns?: boolean;
    bunBuiltIns?: boolean;
    denoBuiltIns?: boolean;
};

export type BaseConfigOptions = {
    externalizeDeps?: ExternalizeOption;
    package?: PackageJson;
    overrides?: UserConfig;
};

export function createConfig(options?: BaseConfigOptions) {
    const rolldownOptions: Rolldown.RolldownOptions = options?.externalizeDeps
        ? {
              external: createExternalizePredicate(options),
              plugins: [esmExternalRequirePlugin()],
          }
        : {};

    return mergeConfig(
        config,
        mergeConfig(
            {
                build: {
                    rolldownOptions,
                },
            } satisfies UserConfig,
            options?.overrides || {},
        ),
    ) as UserConfig;
}

function createExternalizePredicate(options?: BaseConfigOptions) {
    const externalize = options?.externalizeDeps;
    if (typeof externalize === "function") {
        return externalize;
    }

    if (Array.isArray(externalize)) {
        const externalSet = new Set(externalize);
        return (id: string) => externalSet.has(id);
    }

    if (typeof externalize === "object" && options?.package) {
        return createExternalizeDependenciesPredicate(
            externalize,
            options.package,
        );
    }

    if (externalize === true) {
        return createExternalizeDependenciesPredicate(
            {
                dependencies: true,
                devDependencies: true,
                peerDependencies: true,
                bunBuiltIns: true,
                nodeBuiltIns: true,
                denoBuiltIns: true,
            },
            options.package || {},
        );
    }

    return (_id: string) => false;
}

function createExternalizeDependenciesPredicate(
    options: ExternalizeDependencyTypesOption,
    pkg: PackageJson,
): (id: string) => boolean {
    const depsPredicate = createPredicateFromDependencyOption(
        options.dependencies ?? false,
        pkg.dependencies || {},
    );

    const devDepsPredicate = createPredicateFromDependencyOption(
        options.devDependencies ?? false,
        pkg.devDependencies || {},
    );

    const peerDepsPredicate = createPredicateFromDependencyOption(
        options.peerDependencies ?? false,
        pkg.peerDependencies || {},
    );

    const nodeBuiltIns = new Set<string>(
        options.nodeBuiltIns ? builtinModules : [],
    );

    return (id: string) => {
        if (
            (options.denoBuiltIns ??
                options.bunBuiltIns ??
                options.nodeBuiltIns) &&
            (id.startsWith("node:") || id === "node" || nodeBuiltIns.has(id))
        ) {
            return true;
        }

        if (options.bunBuiltIns && (id.startsWith("bun:") || id === "bun")) {
            return true;
        }

        if (options.denoBuiltIns && (id.startsWith("deno:") || id === "deno")) {
            return true;
        }

        if (options.dependencies && depsPredicate(id)) {
            return true;
        }

        if (options.devDependencies && devDepsPredicate(id)) {
            return true;
        }

        if (options.peerDependencies && peerDepsPredicate(id)) {
            return true;
        }

        return false;
    };
}

function createPredicateFromDependencyOption(
    option: DependencyOption,
    dependencies: Record<string, string>,
) {
    if (typeof option === "boolean") {
        const keys = new Set(Object.keys(dependencies));
        return (id: string) => option && keys.has(id);
    }

    if (Array.isArray(option)) {
        const set = new Set(option);

        return (id: string) => set.has(id);
    }

    return option;
}
