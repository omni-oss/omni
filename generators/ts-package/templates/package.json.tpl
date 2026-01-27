{
    "name": "@omni-oss/{{ prompts.package_name }}",
    "version": "{{ prompts.package_version }}",
    "exports": {
        ".": {
            "import": {
                "development": "./src/index.ts",
                "default": "./dist/{{ prompts.package_name }}.mjs"
            },
            "require": {
                "development": "./src/index.ts",
                "default": "./dist/{{ prompts.package_name }}.cjs"
            },
            "types": "./dist/index.d.ts"
        }
    },
    "dependencies": {},
    "devDependencies": {
        "@omni-oss/tsconfig": "workspace:*",
        "@omni-oss/vite-config": "workspace:*",
        "@omni-oss/vitest-config": "workspace:*",
        "vite": "^7.2.2",
        "vitest": "^4.0.8",
        "typescript": "^5.9.3",
        "@types/node": "24.10.1"
    }
}
