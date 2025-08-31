import fs from "node:fs/promises";
import nodePath from "node:path";
import { Command } from "@commander-js/extra-typings";
import { generateRefDocsFromSpec } from "./declspec-docgen";
import { VirtualFileSystem } from "./fs";
import { copyToVfsIfExists } from "./utils";

const program = new Command();

program
    .name("omni-ref-docgen")
    .description("Generate CLI documentation from JSON schema")
    .argument("<spec>", "JSON schema file path")
    .argument("<out>", "Output directory")
    .option("-d, --delete", "delete output directory before generating", true)
    .action(async (spec, out, opts) => {
        const content = await fs.readFile(spec, "utf-8");

        const vfs = new VirtualFileSystem();
        if (opts.delete) {
            copyToVfsIfExists(vfs, "index.mdx", out);
            copyToVfsIfExists(vfs, "meta.json", out);
            copyToVfsIfExists(vfs, nodePath.join("commands", "index.mdx"), out);
            copyToVfsIfExists(vfs, nodePath.join("commands", "meta.json"), out);

            await fs.rm(out, { recursive: true, force: true });
        }

        await generateRefDocsFromSpec(content, vfs, "commands");
        await vfs.writeFilesToDisk(out);
    })
    .parse();
