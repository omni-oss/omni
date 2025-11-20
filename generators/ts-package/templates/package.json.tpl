{
    "name": "@omni-oss/{{ prompts.package_name }}",
    "version": "{{ prompts.package_version }}",
    "exports": {
        ".": "./src/index.ts"
    },
    "dependencies": {},
    "devDependencies": {
        "@omni-oss/tsconfig": "workspace:*",
        "@omni-oss/vite-config": "workspace:*",
        "vite": "^7.2.2",
        "vitest": "^4.0.8",
        "typescript": "^5.9.3",
        "@types/node": "24.10.1"
    }
}
