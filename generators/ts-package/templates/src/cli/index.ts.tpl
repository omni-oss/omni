{% if inputs.shebang_executor == "node" -%}
#!/usr/bin/env node
{% elif inputs.shebang_executor == "bun" -%}
#!/usr/bin/env bun
{% elif inputs.shebang_executor == "deno" -%}
#!/usr/bin/env deno
{% endif %}

import { Command } from "@commander-js/extra-typings";
import packageJson from "../../package.json" with { type: "json" };
import { add } from "../add";

const program = new Command();

const { description, name, version } = packageJson;

program.name(name).version(version).description(description);

program
    .command("add")
    .description("Add two numbers.")
    .argument("<a>", "The first number.")
    .argument("<b>", "The second number.")
    .action(async (a, b) => {
        console.log(`result ${add(Number(a), Number(b))}`);
    });

{%- if inputs.shebang_executor == "node" %}
program.parseAsync();
{%- elif inputs.shebang_executor == "bun" or inputs.shebang_executor == "deno" %}
await program.parseAsync();
{% endif %}
