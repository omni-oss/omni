import { getInput, info, setFailed, setOutput } from "@actions/core";

try {
    // 1. Get and parse the build data JSON input
    const buildDataJson = getInput("build_data_json", { required: true });
    const data = JSON.parse(buildDataJson);

    // Initialize all output variables to 'false' (default is to skip)
    let installRustTools = "false";
    let installJsTools = "false";
    let buildOrcs = "false";
    let buildOmni = "false";

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

    // --- Logic for BUILD_ORCS & BUILD_OMNI (Aggregated by Project) ---
    const projects = data.aggregated_by_project || {};

    // BUILD_ORCS: Check 'omni_remote_cache_service'
    if (needsAction(projects.omni_remote_cache_service)) {
        buildOrcs = "true";
    }

    // BUILD_OMNI: Check 'omni'
    if (needsAction(projects.omni)) {
        buildOmni = "true";
    }

    const buildProjects = [];
    const publishProjects = [];
    for (const [projectName, project] of Object.entries(projects)) {
        if (needsAction(project)) {
            const projectTasks = data.projects[projectName].tasks;

            for (const [_, task] of Object.keys(projectTasks)) {
                if (task.execute) {
                    buildProjects.push(projectName);
                    if (task.meta.publish) {
                        publishProjects.push(projectName);
                    }
                }
            }
        }
    }

    // --- Set Action Outputs ---
    setOutput("INSTALL_RUST_TOOLS", installRustTools);
    setOutput("INSTALL_JS_TOOLS", installJsTools);
    setOutput("BUILD_ORCS", buildOrcs);
    setOutput("BUILD_OMNI", buildOmni);
    setOutput("BUILD_PROJECTS", JSON.stringify(buildProjects));
    setOutput("PUBLISH_PROJECTS", JSON.stringify(publishProjects));

    // Log the final determined values for debugging in the workflow
    info(`INSTALL_RUST_TOOLS set to: ${installRustTools}`);
    info(`INSTALL_JS_TOOLS set to: ${installJsTools}`);
    info(`BUILD_ORCS set to: ${buildOrcs}`);
    info(`BUILD_OMNI set to: ${buildOmni}`);
    info(`BUILD_PROJECTS: ${JSON.stringify(buildProjects, null, 4)}`);
    info(`PUBLISH_PROJECTS: ${JSON.stringify(publishProjects, null, 4)}`);
} catch (error) {
    setFailed(`Action failed: ${error.message}`);
}
