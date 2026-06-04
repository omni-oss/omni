{% if prompts.package_type == 'lib' or prompts.package_type == 'script' -%}
export * from "./add";
{% endif %}
