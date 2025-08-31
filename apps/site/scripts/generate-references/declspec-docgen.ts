import nodePath from "node:path";
import { z } from "zod";
import type { FileSystem } from "./fs";

// Zod schemas based on the JSON schema
const CliArgSchema = z.object({
    id: z.string(),
    aliases: z.array(z.string()),
    required: z.boolean(),
    default_values: z.array(z.string()),
    possible_values: z.array(z.string()),
    short: z.string().length(1).nullable().optional(),
    long: z.string().nullable().optional(),
    help: z.string().nullable().optional(),
    long_help: z.string().nullable().optional(),
    env: z.string().nullable().optional(),
});

const CliCommandSchema = z.object({
    name: z.string(),
    aliases: z.array(z.string()),
    positionals: z.array(CliArgSchema),
    opts: z.array(CliArgSchema),
    get subcommands() {
        return z.array(CliCommandSchema);
    },
    about: z.string().nullable().optional(),
    long_about: z.string().nullable().optional(),
    author: z.string().nullable().optional(),
    bin_name: z.string().nullable().optional(),
    version: z.string().nullable().optional(),
    long_version: z.string().nullable().optional(),
    short_flag: z.string().length(1).nullable().optional(),
    long_flag: z.string().nullable().optional(),
});

// Type inference from Zod schemas
type CliArg = z.infer<typeof CliArgSchema>;
type CliCommand = z.infer<typeof CliCommandSchema>;

// MDX documentation generator
class DeclspecMdxDocGenerator {
    constructor(
        private fs: FileSystem,
        private basePath?: string,
    ) {}

    async generateDocs(command: CliCommand, basePath: string = "") {
        await this.generateCommandDocs([], command, basePath);
    }

    private async generateCommandDocs(
        parents: string[],
        command: CliCommand,
        currentPath: string,
    ) {
        const commandPath = currentPath
            ? `${currentPath}/${command.name}`
            : command.name;

        const p = parents.concat(command.name);

        if (command.subcommands.length > 0) {
            // Command has subcommands - create folder with index.mdx
            const indexPath = `${commandPath}/index.mdx`;

            const indexContent = this.generateCommandMdx(
                parents,
                command,
                true,
            );
            await this.writeFile(indexPath, indexContent);

            // Generate docs for each subcommand
            for (const subcommand of command.subcommands) {
                this.generateCommandDocs(p, subcommand, commandPath);
            }
        } else {
            // Leaf command - create individual mdx file
            const filePath = `${commandPath}.mdx`;
            const content = this.generateCommandMdx(parents, command, false);
            await this.writeFile(filePath, content);
        }
    }

    private async writeFile(path: string, content: string) {
        if (this.basePath) {
            path = nodePath.join(this.basePath, path);
        }
        await this.fs.writeFile(path, content);
    }

    private escapeHtml(str: string): string {
        return str
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#39;");
    }

    private generateCommandMdx(
        parents: string[],
        command: CliCommand,
        hasSubcommands: boolean,
    ): string {
        const mdx = [];

        // Frontmatter
        mdx.push("---");
        mdx.push(`title: "${command.name}"`);
        if (command.about) {
            mdx.push(`description: "${command.about}"`);
        }
        if (command.version) {
            mdx.push(`version: "${command.version}"`);
        }
        if (command.author) {
            mdx.push(`author: "${this.escapeHtml(command.author)}"`);
        }
        mdx.push("---");
        mdx.push("");

        // Title and description
        // mdx.push(`# ${command.name}`);
        // mdx.push("");

        if (command.long_about && command.long_about !== command.about) {
            mdx.push(command.long_about);
        }

        // Version and author info
        // const metadata = [];
        // if (command.version) metadata.push(`**Version:** ${command.version}`);
        // if (command.author)
        //     metadata.push(`**Author:** ${this.escapeHtml(command.author)}`);
        // if (command.bin_name) metadata.push(`**Binary:** ${command.bin_name}`);

        // if (metadata.length > 0) {
        //     mdx.push(...metadata);
        //     mdx.push("");
        // }

        // Aliases
        if (command.aliases.length > 0) {
            mdx.push("---");
            mdx.push("## Aliases");
            mdx.push(`\`${command.aliases.join("`, `")}\``);
        }

        // Usage section
        mdx.push("## Usage");
        mdx.push(this.generateUsageCodeBlock(parents, command));

        // Positional arguments
        if (command.positionals.length > 0) {
            mdx.push("---");
            mdx.push("## Positional arguments");
            for (const arg of command.positionals) {
                mdx.push(this.generateArg(arg, true));
            }
        }

        // Options
        if (command.opts.length > 0) {
            mdx.push("---");
            mdx.push("## Options");
            for (const opt of command.opts) {
                mdx.push(this.generateArg(opt));
            }
        }

        // Subcommands section (for parent commands)
        if (hasSubcommands) {
            mdx.push("---");
            mdx.push("## Subcommands");
            for (const subcommand of command.subcommands) {
                const subcommandLink = `./${subcommand.name}`;
                mdx.push(
                    `- [\`${subcommand.name}\`](${subcommandLink}) - ${subcommand.about || "No description"}`,
                );
            }
        }

        mdx.push("---");

        // Examples section (placeholder)
        mdx.push("## Examples");
        mdx.push(this.generateUsageCodeBlock(parents, command));

        mdx.push("---");

        return mdx.join("\n\n");
    }

    private generateArg(arg: CliArg, positional = false): string {
        const mdx = [];
        const flags = [];

        if (positional) {
            const argName = arg.required ? `<${arg.id}>` : `[${arg.id}]`;
            mdx.push(`### \`${argName}\``);
        } else {
            if (arg.short) flags.push(`-${arg.short}`);
            if (arg.long) flags.push(`--${arg.long}`);
            if (arg.aliases.length > 0) flags.push(...arg.aliases);

            const flagsStr =
                flags.length > 0
                    ? flags.map((f) => `\`${f}\``).join(", ")
                    : `\`--${arg.id}\``;

            mdx.push(`### ${flagsStr}`);
        }
        
        const help = arg.long_help || arg.help;

        if (help) {
            mdx.push(help);
        }

        const details = [];
        details.push(`- **Required:** ${arg.required ? "Yes" : "No"}`);

        if (arg.default_values.length > 0) {
            details.push(`- **Default:** \`${arg.default_values.join(", ")}\``);
        }

        if (arg.possible_values.length > 0) {
            details.push(
                `- **Possible values:** \`${arg.possible_values.join("`, `")}\``,
            );
        }

        if (arg.env) {
            details.push(`- **Environment variable:** \`${arg.env}\``);
        }

        mdx.push(...details);

        return mdx.join("\n\n");
    }

    private generateUsageCodeBlock(
        parents: string[],
        command: CliCommand,
    ): string {
        return `\`\`\`bash\n${this.generateUsage(parents, command)}\n\`\`\``;
    }

    private generateUsage(parents: string[], command: CliCommand): string {
        const parts = parents.concat(command.name);

        if (command.opts.length > 0) {
            parts.push("[OPTIONS]");
        }

        if (command.positionals.length > 0) {
            for (const pos of command.positionals) {
                const argName = pos.required ? `<${pos.id}>` : `[${pos.id}]`;
                parts.push(argName);
            }
        }

        if (command.subcommands.length > 0) {
            parts.push("<SUBCOMMAND>");
        }

        return parts.join(" ");
    }

    async getGeneratedFiles(): Promise<Record<string, string>> {
        const files: Record<string, string> = {};
        for (const path of await this.fs.listFiles()) {
            const content = await this.fs.readFile(path);
            if (content) {
                files[path] = content;
            }
        }
        return files;
    }
}

// Parse and validate CLI command with detailed error reporting
function parseCliCommand(command: unknown): CliCommand {
    const json = typeof command === "string" ? JSON.parse(command) : command;
    const pased = CliCommandSchema.safeParse(json);

    if (pased.success) {
        return pased.data;
    } else {
        throw pased.error;
    }
}

// Example usage function with Zod validation
export async function generateRefDocsFromSpec(
    cliSpec: unknown,
    fs: FileSystem,
    basePath?: string,
) {
    let validatedSpec: CliCommand;

    try {
        validatedSpec = parseCliCommand(cliSpec);
    } catch (error) {
        if (error instanceof z.ZodError) {
            console.error("❌ CLI specification validation failed:");
            const errors = z.treeifyError(error);

            for (const err of errors.errors) {
                console.error(err);
            }
        }
        throw new Error("Invalid CLI specification format");
    }

    validatedSpec.name = "omni";
    validatedSpec.bin_name = "omni";

    const generator = new DeclspecMdxDocGenerator(fs, basePath);
    await generator.generateDocs(validatedSpec);
}
