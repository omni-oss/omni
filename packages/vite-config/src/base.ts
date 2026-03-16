import { builtinModules } from "node:module";
import { mergeConfig, type UserConfig } from "vite";
import type { PackageJson } from "./types";

const config: UserConfig = {
    plugins: [],
    resolve: {
        tsconfigPaths: true,
    },
};

export default config;

export type ExternalizeOption =
    | boolean
    | string[]
    | ((id: string) => boolean)
    | ExternalizeDependencyTypesOption;

export type ExternalizeDependencyTypesOption = {
    dependencies?: boolean;
    devDependencies?: boolean;
    peerDependencies?: boolean;
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
    return mergeConfig(
        config,
        mergeConfig(
            {
                build: {
                    rolldownOptions: {
                        external: createExternalizePredicate(options),
                    },
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
    const deps = new Set<string>(
        Object.keys(options.dependencies ? pkg.dependencies || {} : {}),
    );

    const devDeps = new Set<string>(
        Object.keys(options.devDependencies ? pkg.devDependencies || {} : {}),
    );

    const peerDeps = new Set<string>(
        Object.keys(options.peerDependencies ? pkg.peerDependencies || {} : {}),
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

        if (options.dependencies && deps.has(id)) {
            return true;
        }

        if (options.devDependencies && devDeps.has(id)) {
            return true;
        }

        if (options.peerDependencies && peerDeps.has(id)) {
            return true;
        }

        return false;
    };
}
