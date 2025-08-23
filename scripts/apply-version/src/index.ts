import walk from "ignore-walk";

const start = Date.now();
const files = await walk({
    ignoreFiles: [".omniignore", ".gitignore", ".npmignore", ".dockerignore"],
    includeEmpty: false,
    path: process.cwd(),
    follow: false,
});

const FILES_TO_UPDATE = ["package.json", "Cargo.toml"];
const REPLACEMENT_PATTERN = /0.0.0-semantically-released/g;

let currentDir = process.cwd();

while (!(await Bun.file(`${currentDir}/.version`).exists())) {
    if (currentDir === "/") {
        console.error("Could not find .version file");
        process.exit(1);
    }

    currentDir = currentDir.replace(/\/[^/]+$/, "");
}

const versionText = await Bun.file(`${currentDir}/.version`).text();

const VERSION_REGEX =
    /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-((?:0|[1-9][0-9]*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9][0-9]*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$/;

if (!VERSION_REGEX.test(versionText)) {
    console.error(`Invalid version format: ${versionText}`);
    process.exit(1);
}

for (const file of files) {
    for (const fileToUpdate of FILES_TO_UPDATE) {
        if (file.endsWith(fileToUpdate)) {
            const content = await Bun.file(file).text();

            if (REPLACEMENT_PATTERN.test(content)) {
                const updatedContent = content.replace(
                    REPLACEMENT_PATTERN,
                    versionText,
                );
                await Bun.file(file).write(updatedContent);
                console.log(`Applied version in ${file}`);
            }
        }
    }
}

const end = Date.now();
const elapsed = end - start;
console.log(`Applied version in ${elapsed}ms`);
