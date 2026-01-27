{
    "extends": ["./tsconfig.json"],
    "compilerOptions": {
        "declarationDir": "./dist/types",
        "declarationMap": true,
        "declaration": true,
        "emitDeclarationOnly": true
    },
    "include": ["${configDir}/src/**/*.*"],
    "exclude": ["${configDir}/src/**/*.{spec,test}.*"]
}
