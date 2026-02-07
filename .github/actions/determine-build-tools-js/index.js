import { getInput, info, setFailed, setOutput } from "@actions/core";

try {
    // 1. Get and parse the build data JSON input
    const buildDataJson = getInput("build_data_json", { required: true });
    const data = JSON.parse(buildDataJson);

    // Initialize all output variables to 'false' (default is to skip)
    let installRustTools = "false";
    let installJsTools = "false";

    // Define the condition for *needing* an action: when the completed value is greater than the completed_with_cache_hit value
    const needsAction = (item) => {
        return item && item.completed > item.completed_with_cache_hit;
    };

    // --- Logic for INSTALL_RUST_TOOLS & INSTALL_JS_TOOLS (Aggregated by Metadata) ---
    const metadata = data.aggregated_by_metadata || {};

    // INSTALL_RUST_TOOLS: Check 'language:rust'
    if (needsAction(metadata["language:rust"])) {
        installRustTools = "true";
    }

    // INSTALL_JS_TOOLS: Check 'language:typescript'
    if (needsAction(metadata["language:typescript"])) {
        installJsTools = "true";
    }

    // --- Set Action Outputs ---
    setOutput("INSTALL_RUST_TOOLS", installRustTools);
    setOutput("INSTALL_JS_TOOLS", installJsTools);

    // Log the final determined values for debugging in the workflow
    info(`INSTALL_RUST_TOOLS set to: ${installRustTools}`);
    info(`INSTALL_JS_TOOLS set to: ${installJsTools}`);
} catch (error) {
    setFailed(`Action failed: ${error.message}`);
}
