const core = require('@actions/core');

try {
  // 1. Get and parse the build data JSON input
  const buildDataJson = core.getInput('build_data_json', { required: true });
  const data = JSON.parse(buildDataJson);

  // Initialize all output variables to 'false' (default is to skip)
  let installRustTools = 'false';
  let installJsTools = 'false';
  let buildOrcs = 'false';
  let buildOmni = 'false';

  // Define the condition for *needing* an action: when the completed value is greater than the completed_with_cache_hit value
  const needsAction = (item) => {
    return item && item.completed > item.completed_with_cache_hit;
  };

  // --- Logic for INSTALL_RUST_TOOLS & INSTALL_JS_TOOLS (Aggregated by Metadata) ---
  const metadata = data.aggregated_by_metadata || {};

  // INSTALL_RUST_TOOLS: Check 'language:rust'
  if (needsAction(metadata['language:rust'])) {
    installRustTools = 'true';
  }

  // INSTALL_JS_TOOLS: Check 'language:typescript'
  if (needsAction(metadata['language:typescript'])) {
    installJsTools = 'true';
  }

  // --- Logic for BUILD_ORCS & BUILD_OMNI (Aggregated by Project) ---
  const projects = data.aggregated_by_project || {};

  // BUILD_ORCS: Check 'omni_remote_cache_service'
  if (needsAction(projects['omni_remote_cache_service'])) {
    buildOrcs = 'true';
  }

  // BUILD_OMNI: Check 'omni'
  if (needsAction(projects['omni'])) {
    buildOmni = 'true';
  }

  // --- Set Action Outputs ---
  core.setOutput('INSTALL_RUST_TOOLS', installRustTools);
  core.setOutput('INSTALL_JS_TOOLS', installJsTools);
  core.setOutput('BUILD_ORCS', buildOrcs);
  core.setOutput('BUILD_OMNI', buildOmni);

  // Log the final determined values for debugging in the workflow
  core.info(`INSTALL_RUST_TOOLS set to: ${installRustTools}`);
  core.info(`INSTALL_JS_TOOLS set to: ${installJsTools}`);
  core.info(`BUILD_ORCS set to: ${buildOrcs}`);
  core.info(`BUILD_OMNI set to: ${buildOmni}`);

} catch (error) {
  core.setFailed(`Action failed: ${error.message}`);
}
