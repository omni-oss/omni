# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## omni_cli_core-v0.17.2 - 2026-07-09
#### Bug Fixes
- (**@omni-oss/workspace-gen**) copy dist from rust wasm package - (1bba298) - Clarence Manuel
- (**omni_generator**) add_many ignoring hidden files - (2f2cc69) - Clarence Manuel
- (**omni_generator**) improve error message for deserialization error - (858460c) - Clarence Manuel
- (**omni_workspace_gen**) build script for wasm-pack - (c342744) - Clarence Manuel
- bug in task_execution_graph for sibling tasks - (c658103) - Clarence Manuel
#### Performance Improvements
- improve performance in collection code - (5ed50d1) - Clarence Manuel
- improve performance by removing unnecessary statx calls - (56efa53) - Clarence Manuel
#### Tests
- (**omni_bench**) implement benchmark crate - (b085d32) - Clarence Manuel
- (**omni_expressions**) update failing tests - (bdb6d7e) - Clarence Manuel
- (**omni_test_utils**) refactor test utils - (de096d6) - Clarence Manuel
- update/add benchmarks - (6cd34dd) - Clarence Manuel
#### Refactoring
- expose more walkdir options in dir_walker - (98973ca) - Clarence Manuel
- implement omni_workspace_gen common crate - (e81bde3) - Clarence Manuel
#### Miscellaneous Chores
- add .npmignore to ts packages [skip ci] - (38a0498) - Clarence Manuel

- - -

## omni_cli_core-v0.17.1 - 2026-07-05
#### Bug Fixes
- incorrect generator schema - (16ee36e) - Clarence Manuel

- - -

## omni_cli_core-v0.17.0 - 2026-07-04
#### Features
- implement progress ui for task execution and add  --output-logs, --output-cached-logs flags - (9e7e679) - Clarence Manuel
- upgrade tera version and update presets - (c097dc8) - Clarence Manuel
- remove default_expr and support Object input - (8f7e84a) - Clarence Manuel
#### Bug Fixes
- (**omni_cli_core**) update loading text on retry - (3176613) - Clarence Manuel
- (**omni_cli_core**) deduplicate paths in generator reporting - (5d63f3d) - Clarence Manuel
- (**omni_cli_core**) update path diff on generator report - (76a41dd) - Clarence Manuel
- (**omni_cli_core**) improve generator output summary - (e6b4ca5) - Clarence Manuel
- (**omni_cli_core**) print output summary after generator completes - (1f1bdcf) - Clarence Manuel
- (**omni_cli_core**) rename stdout-logs-level to stdout-log-level - (9241915) - Clarence Manuel
- (**omni_configurations**) improve error messages in load_config - (3f54c90) - Clarence Manuel
- (**omni_generator**) utilize new generator events and convert info logs to debug logs - (5d588ce) - Clarence Manuel
- (**omni_generator**) TransactionSys info logs should be debug - (20dc6dc) - Clarence Manuel
- (**omni_messages**) add additional generator events - (791d4c8) - Clarence Manuel
- (**omni_tera**) add matching and regex_replace test to preset - (8df6432) - Clarence Manuel
- skip serializing null values - (e25f20a) - Clarence Manuel
- move output configuration under cache field - (ed633c0) - Clarence Manuel
- flaky task success should supersede earlier failure - (a912b54) - Clarence Manuel
- apply deny_unknown_fields for configuration types - (3acf7b6) - Clarence Manuel
- broken tui mode - (b055007) - Clarence Manuel
- rename FILENAME to FILEPATH environmental variable in transform-many action - (7f7eb0c) - Clarence Manuel
- use progress indicators for generator progress - (2022354) - Clarence Manuel
- swap serde_norway for noyalib and improve serialization-related error reporting - (7c65947) - Clarence Manuel
- rename and refactoring of input mechanism for generator - (cb8fa22) - Clarence Manuel
#### Refactoring
- (**omni_generator**) rename utility methods - (8def5d5) - Clarence Manuel
- create builder extensions - (635d671) - Clarence Manuel
- use builder for creating inputs - (e59d909) - Clarence Manuel
#### Miscellaneous Chores
- format vitest config files - (019f6bd) - Clarence Manuel
- update configuration to publish js packages - (f46a1de) - Clarence Manuel

- - -

## omni_cli_core-v0.16.0 - 2026-06-21
#### Features
- (**@omni-oss/bridge-rpc-services**) implement BridgeRpcSystem - (74a4e07) - Clarence Manuel
- (**@omni-oss/bridge-rpc-system-interface**) refactor system interface implementation into it's own package - (43044bd) - Clarence Manuel
- (**@omni-oss/bridge-service**) initial bridge service implementation - (a68f36c) - Clarence Manuel
- (**@omni-oss/channels**) implement bounded mspc and cleanup oneshot code - (5e4db7f) - Clarence Manuel
- (**@omni-oss/gen-sdk**) expose gen-sdk package - (d561790) - Clarence Manuel
- (**@omni-oss/gen-sdk-core**) add gen-sdk-core package - (0560c6c) - Clarence Manuel
- (**@omni-oss/log**) implement centralized logging facade library - (d618a12) - Clarence Manuel
- (**omni_generator**) implement transform and transform-many actions - (f14e39f) - Clarence Manuel
- convert --stderr-log-enabled flag to --stderr-log-evel - (d99be87) - Clarence Manuel
- add initial mcp support - (a5c8b6e) - Clarence Manuel
- implement run-javascript action - (7800281) - Clarence Manuel
- implement meta --meta filter on cache prune and stats - (d019202) - Clarence Manuel
- add transform and transform-many configuration - (3e1dc5b) - Clarence Manuel
- implement bridge-rpc utils and services - (121791b) - Clarence Manuel
#### Bug Fixes
- (**@omni-oss/async-utils**) implement async-context utilities - (abe77dd) - Clarence Manuel
- (**@omni-oss/bridge-rpc**) implement async-context propagation in AbstractTransport.onReceive - (5a4422d) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) improve error handling - (181bac2) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) fix transport-read-framer bug and improve error handling and propagation - (9ec7d72) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) implement frame tuple wire format - (84ec442) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) improve error handling on BridgeRpc.stop - (fe924d4) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) status code visibility issue - (b54edc4) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) implement instance caching for status code classes - (53c2aef) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) add asyncDispose cleanup support for request and response objects - (403f47e) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) use interface instead of type for Service type - (3751cc1) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) add equals method to status code classes - (f0cb3d1) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) allow method chaining on writeBodyChunk methods - (42f75d0) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) add state check assertions for request and response objects - (e45545c) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) expose ClientHandle to services - (842cce1) - Clarence Manuel
- (**@omni-oss/bridge-rpc-router**) improve error handling Router.run method - (e5ea40a) - Clarence Manuel
- (**@omni-oss/bridge-rpc-service**) utilize BridgeRpcSystem in dry-run and normal mode - (a829f2b) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) utilize gen-sdk-core package - (25bd3d0) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) windows path handling - (db8ff22) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) support outputDir generator script context variable and in-memory cwd - (9fa3957) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) update BridgeRpcSystem.env() return type to ArgsList - (1aeb52c) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) update build dependencies - (5e9c123) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) move exec-generator-script implementation - (0714d8c) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) utilize new package and changes and rename loadScript to importScript - (8279bd8) - Clarence Manuel
- (**@omni-oss/bridge-rpc-utils**) implement fail server response function - (be1de22) - Clarence Manuel
- (**@omni-oss/bridge-service**) fix undefined ClientHandle - (4a7bf16) - Clarence Manuel
- (**@omni-oss/bridge-service**) update failing test - (e2e1c6c) - Clarence Manuel
- (**@omni-oss/bridge-service**) add sink implementation - (c7ccae6) - Clarence Manuel
- (**@omni-oss/bridge-services**) use method instead of field for ExecScript implementation - (15f8c84) - Clarence Manuel
- (**@omni-oss/channels**) prevent Oneshot.receive from being called twice - (618655d) - Clarence Manuel
- (**@omni-oss/console-log-interceptor**) implement console log interception - (7709a0b) - Clarence Manuel
- (**@omni-oss/gen-sdk-core**) export additional types - (bdf4740) - Clarence Manuel
- (**@omni-oss/log**) update export maps - (509d7d1) - Clarence Manuel
- (**@omni-oss/log**) implement noop logging as fallback logger - (5869e02) - Clarence Manuel
- (**@omni-oss/runtime-utils**) create new package runtime related utilities - (e264ee3) - Clarence Manuel
- (**@omni-oss/system-interface**) make ProcessEnv and ArgsList readonly - (14c46a0) - Clarence Manuel
- (**@omni-oss/system-interface**) expose implementation entrypoint - (a4d22e2) - Clarence Manuel
- (**@omni-oss/system-interface**) resolve build error - (e00c2e8) - Clarence Manuel
- (**@omni-oss/vite-config**) add additional configuration for library preset - (3eecf9c) - Clarence Manuel
- (**@omni-oss/vite-config**) add bundleTypes option to library preset - (440b766) - Clarence Manuel
- (**@omni-oss/vite-config**) add esmExternalRequirePlugin to base config - (4a55619) - Clarence Manuel
- (**@omni-oss/vite-config**) add test preset - (9c0e606) - Clarence Manuel
- (**@omni/bridge-service**) improve error log handling propagation - (036d791) - Clarence Manuel
- (**bridge_rpc_core**) improve error handling - (5600ed3) - Clarence Manuel
- (**bridge_rpc_core**) fix incorrect serialization of headers due to rmpv::ext::to_value handling - (86cb1a3) - Clarence Manuel
- (**bridge_rpc_core**) fix transport_read_framer bug and improve error handling and propagation - (74f0479) - Clarence Manuel
- (**bridge_rpc_core**) implement frame wire format as tuple - (533b8d0) - Clarence Manuel
- (**bridge_rpc_core**) improve DynMap methods implementation - (a947334) - Clarence Manuel
- (**bridge_rpc_core**) add into_trailers method for request and response - (802ee64) - Clarence Manuel
- (**bridge_rpc_services**) fix incorrect serialization of headers due to rmpv::ext::to_value handling - (5659d19) - Clarence Manuel
- (**bridge_rpc_services**) implement proc and fs services - (a43afdb) - Clarence Manuel
- (**bridge_rpc_services**) implement initial services crate for bridge_rpc - (c46d2f6) - Clarence Manuel
- (**bridge_rpc_utils**) implement utils crate for bridge_rpc - (7805a19) - Clarence Manuel
- (**js_runtime**) windows path handling - (e4549d3) - Clarence Manuel
- (**omni_api**) implement deep generator inspect and input validate - (8ac1c94) - Clarence Manuel
- (**omni_api**) add generator_inspect - (f5985b3) - Clarence Manuel
- (**omni_cache**) implement locking for concurrent runs - (02c5446) - Clarence Manuel
- (**omni_cli_core**) validation failures should return non-zero exit code - (6a48207) - Clarence Manuel
- (**omni_cli_core**) support --git-rev flag in init subcommand - (eb55840) - Clarence Manuel
- (**omni_cli_core**) remove short in --max-depth flag - (da68bcd) - Clarence Manuel
- (**omni_cli_core**) support --max-depth flag in generator subcommand - (42d3dd2) - Clarence Manuel
- (**omni_cli_core**) use total time saved field - (00a11cc) - Clarence Manuel
- (**omni_cli_core**) any scm flags should implicitly enable scm filtering - (5845ddd) - Clarence Manuel
- (**omni_cli_core**) exec shouldn't take empty command - (224fb8f) - Clarence Manuel
- (**omni_cli_core**) fix mismatched arguments position for prune command - (587a8b6) - Clarence Manuel
- (**omni_cli_core**) ignore-session and save-session flags should optionally take values - (e26e888) - Clarence Manuel
- (**omni_context**) add ensure_loaded method to MaybeLoaded - (94b1bf5) - Clarence Manuel
- (**omni_context**) incorrect ignore files configuration - (a2c9633) - Clarence Manuel
- (**omni_execution_plan**) apply filters dir and meta filters on exec path - (8ef9924) - Clarence Manuel
- (**omni_generator**) add max_depth support - (a2c6040) - Clarence Manuel
- (**omni_generator**) add recursion detection - (36c32ae) - Clarence Manuel
- (**omni_generator**) serialize generator runs when run in parallel - (478cbd6) - Clarence Manuel
- (**omni_generator**) add user_invocable flag for generator configuration - (39440c9) - Clarence Manuel
- (**omni_generator**) output_dir in context is not relative on windows - (dc49c99) - Clarence Manuel
- (**omni_generator**) --use-defaults flag doesn't propagate to sub-generators - (cb8ceaf) - Clarence Manuel
- (**omni_generator**) rename prompts to inputs - (cc32919) - Clarence Manuel
- (**omni_generator**) windows path handling - (9acbfd5) - Clarence Manuel
- (**omni_generator**) provide output_dir to run-javascript actions - (915cf6e) - Clarence Manuel
- (**omni_generator**) fix typo in ignore files configuration - (3f29ef7) - Clarence Manuel
- (**omni_generator**) normalize paths in transaction_sys methods - (52c8217) - Clarence Manuel
- (**omni_generator**) make inner Sys implementation shared - (4ae89d9) - Clarence Manuel
- (**omni_generator**) implemenet transactional system - (84f1422) - Clarence Manuel
- (**omni_generator**) fix DryRunSys compile errors - (a028572) - Clarence Manuel
- (**omni_generator_configuration**) hide run-command action - (d189f60) - Clarence Manuel
- (**omni_generator_configurations**) rename prompts to inputs - (28f910f) - Clarence Manuel
- (**omni_hasher**) fix UnexpectedEnd error on concurrent omni execution - (e1ea8b7) - Clarence Manuel
- (**omni_input_provider**) validate function not getting processed value - (97a8d00) - Clarence Manuel
- (**omni_input_provider**) preset values not available in if conditions - (60c2ca6) - Clarence Manuel
- (**omni_mcp_core**) expose more parameter fields to cache pruning - (d797b68) - Clarence Manuel
- (**omni_mcp_core**) update generator tools - (a1cfbe1) - Clarence Manuel
- (**omni_mcp_core**) update generator_run tool description - (c3567c3) - Clarence Manuel
- (**omni_messages**) add total time saved calculation field - (fbcc278) - Clarence Manuel
- (**omni_prompt**) implement proper runtime typecheck for prompt values - (157b928) - Clarence Manuel
- (**omni_task_executor**) populate total time saved calculation - (659a5c8) - Clarence Manuel
- (**omni_utils**) add LockGuard file lock - (e5010ad) - Clarence Manuel
- (**system_traits**) implement more traits for InMemorySys - (c7faa16) - Clarence Manuel
- (**system_traits**) add and implement new traits - (c35fd03) - Clarence Manuel
- expose description field in generator inputs - (46bc836) - Clarence Manuel
- prevent double loading of context - (2702b3a) - Clarence Manuel
- rename prompts to inputs - (516dba6) - Clarence Manuel
- prune subcommand crash due to duplicate flag short name - (dfff39c) - Clarence Manuel
- rename bridge_rpc crate to bridge_rpc_core and expose a ClientHandle instance to services - (cbb4e9d) - Clarence Manuel
- support fine-grained predicates for dependency externalize options - (9367b4e) - Clarence Manuel
- support single path input for exec-script service - (e34f360) - Clarence Manuel
- add validation for status code classes - (99ea27d) - Clarence Manuel
#### Tests
- (**@omni-oss/bridge-rpc-services**) fix lint errors in js test fixtures - (1471860) - Clarence Manuel
- (**@omni-oss/bridge-service**) update integration test payload - (49a8353) - Clarence Manuel
- (**@omni-tests**) fix failing tests on windows - (5221988) - Clarence Manuel
- update broken tests - (05d3582) - Clarence Manuel
- add unit tests to exec-generator-script - (0eebf99) - Clarence Manuel
#### Refactoring
- (**@omni-oss/bridge-service**) move files into other packages - (f0808ac) - Clarence Manuel
- (**omni_api**) utilize ensure_loaded - (133c099) - Clarence Manuel
- (**omni_cache**) utilize omni_utils::lock::LockGuard - (e76a1ed) - Clarence Manuel
- (**omni_generator**) make GenSession methods async - (26640a8) - Clarence Manuel
- (**omni_input_provider**) add common function as method in configuration - (2542b2e) - Clarence Manuel
- (**omni_lockfile**) use async mutex for data handling - (fca88f2) - Clarence Manuel
- introduce and use input builders - (25221a0) - Clarence Manuel
- update parameter type - (b21493b) - Clarence Manuel
- allow optionally passing arbitrary version file to bridge service vendoring - (56d3912) - Clarence Manuel
- expose base method for ActionConfiguration - (58decdc) - Clarence Manuel
- create a central omni_api and omni_messages crate extracted from omni_cli_core - (eba8ec1) - Clarence Manuel
- improve omni_generator architecture and testability - (487a134) - Clarence Manuel
- configurations - (3b140ea) - Clarence Manuel
#### Miscellaneous Chores
- (**@omni-oss/bridge-rpc-bootstrap**) rename package - (dcb88dd) - Clarence Manuel
- (**@omni-oss/bridge-rpc-core**) update integration tests - (f33d4b3) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) remove old rpc-system files - (53e8b78) - Clarence Manuel
- (**@omni-oss/bridge-rpc-services**) update DryRunSystem mock implementations - (77c1465) - Clarence Manuel
- (**@omni-oss/bridge-rpc-utils**) fix dependency error - (f419798) - Clarence Manuel
- (**@omni-oss/bridge-service**) fix compile error due to missing dependencies - (ddf5ef1) - Clarence Manuel
- (**@omni-oss/bridge-service**) fix output artifact not runnable on runtimes without require support - (c98983a) - Clarence Manuel
- (**@omni-oss/bridge-service**) update integration test - (f18018a) - Clarence Manuel
- (**@omni-oss/bridge-service**) remove unused package config - (aaa0754) - Clarence Manuel
- (**@omni-oss/bridge-service**) fix tests - (6da3d54) - Clarence Manuel
- (**@omni-oss/bridge-service-tests**) initial implementation bridge service tests - (62d4616) - Clarence Manuel
- (**@omni-oss/console-log-interceptor**) fix unresolved module on ci - (203d6c7) - Clarence Manuel
- (**@omni-oss/vitest-config**) add vitest coverage support - (da6d494) - Clarence Manuel
- (**@omni-oss/vitest-config**) rename api config to service config - (aa0f58e) - Clarence Manuel
- (**bridge_rpc_utils**) update tests - (2212b41) - Clarence Manuel
- (**js_runtime**) minor changes - (7857e98) - Clarence Manuel
- (**omni_generator**) update DryRunSys implementation - (dc4486a) - Clarence Manuel
- add JsonSchema derives - (526dcd1) - Clarence Manuel
- fix typo in project configs - (b610667) - Clarence Manuel
- improve error message for pipe - (4f71e18) - Clarence Manuel
- fix ts build errors - (45a6b32) - Clarence Manuel
- format files - (3b49082) - Clarence Manuel
- add retries - (b2d4616) - Clarence Manuel
- update concurrent request amount in integration tests - (a38c527) - Clarence Manuel
- update project configs - (fee5066) - Clarence Manuel
- update packages versions and configurations - (bb47909) - Clarence Manuel

- - -

## omni_cli_core-v0.15.1 - 2026-05-24
#### Bug Fixes
- generator tera instance have no access to custom filters - (b8ff87b) - Clarence Manuel
- update omni logs path - (3be2dd2) - Clarence Manuel
- update trace and log handling - (7d46766) - Clarence Manuel
- generator sources lock not removing stale git sources - (905b8dc) - Clarence Manuel
- support multiple paths for local generator source - (8757b00) - Clarence Manuel
- local generator source entries sharing the same scope - (7fc821f) - Clarence Manuel

- - -

## omni_cli_core-v0.15.0 - 2026-05-22
#### Features
- add and utilize standard values for tera context - (92c8e38) - Clarence Manuel
- add platform environmantl variables - (2fc88d0) - Clarence Manuel
- add project command - (8d0c325) - Clarence Manuel
#### Bug Fixes
- failure to expand default values - (6a8e45d) - Clarence Manuel
- relative_path filter not working properly on windows - (d58a8ab) - Clarence Manuel
- bug in windows incorrectly resolving the topmost dir to traverse - (6104fef) - Clarence Manuel
#### Miscellaneous Chores
- fix tests on windows - (ede15e6) - Clarence Manuel
- fix tests not finding the correct binary for windows - (ccecf25) - Clarence Manuel
- improve diagnostics messages during test failures - (6403c3c) - Clarence Manuel
- update omni project configs - (d4f4be9) - Clarence Manuel

- - -

## omni_cli_core-v0.14.0 - 2026-05-18
#### Features
- support remote git generator sources - (c83d835) - Clarence Manuel
- update init command to require primary generator config - (a035443) - Clarence Manuel
- implement init subcommand - (ba0e005) - Clarence Manuel
#### Bug Fixes
- swap db-keystore with internal fallback implementation - (f9bb15b) - Clarence Manuel
- add keyring fallback to platforms without pre-installed dependencies - (4c85102) - Clarence Manuel
- tera error due to name mismatch - (b004de8) - Clarence Manuel
- ignore not found io errors - (7e78bd4) - Clarence Manuel
- improve trace formatting - (0d468d8) - Clarence Manuel
- rename command to exec - (7384f6e) - Clarence Manuel
#### Continuous Integration
- add new projects to cog.toml - (f469997) - Clarence Manuel
#### Refactoring
- swap git2 with gix - (80758d4) - Clarence Manuel
- use log for user facing logs - (4ddf7c5) - Clarence Manuel
- utilize new rust dependency APIs - (5191bfb) - Clarence Manuel
#### Miscellaneous Chores
- fix flaky tests - (8717b2f) - Clarence Manuel
- add vendored-opennssl feature to omni binary crate - (2d2bead) - Clarence Manuel
- fix compile errors - (7979672) - Clarence Manuel
- fix omni_setup compilation error - (e046ebd) - Clarence Manuel
- update omni configs json schema links [skip ci] - (d484be7) - Clarence Manuel

- - -

## omni_cli_core-v0.13.0 - 2026-03-21
#### Features
- implement --arg flag for passing custom args - (25d09a9) - Clarence Manuel
- implement --use-defaults flag for generator - (57e8255) - Clarence Manuel
- implement retry_command - (d2f1b3a) - Clarence Manuel

- - -

## omni_cli_core-v0.12.0 - 2026-03-14
#### Features
- support overriding output_dir on run_generator actions - (c5524a7) - Clarence Manuel
#### Bug Fixes
- auto UI mode bypasses the in memory tracing - (b20ba37) - Clarence Manuel
#### Miscellaneous Chores
- fix tests - (27a1134) - Clarence Manuel
- update cel crate version - (a58d1b7) - Clarence Manuel

- - -

## omni_cli_core-v0.11.0 - 2026-02-25
#### Features
- support multiple entries for modify-* actions - (196c184) - Clarence Manuel
- support multiple entries for prepend and append actions - (039388d) - Clarence Manuel

- - -

## omni_cli_core-v0.10.0 - 2026-02-19
#### Features
- support auto tui mode - (db6e135) - Clarence Manuel
#### Bug Fixes
- (**omni_term_ui**) vte performance issue - (64eea06) - Clarence Manuel

- - -

## omni_cli_core-v0.9.3 - 2026-02-17
#### Bug Fixes
- (**omni_task_executor**) error when trace folder doesn't exist - (a8de21d) - Clarence Manuel

- - -

## omni_cli_core-v0.9.2 - 2026-02-17
#### Bug Fixes
- (**omni_generator**) dry run write error - (fa3d46f) - Clarence Manuel
#### Miscellaneous Chores
- update dependencies - (cbdbb59) - Clarence Manuel

- - -

## omni_cli_core-v0.9.1 - 2026-02-16
#### Bug Fixes
- error when cache is disabled for a task - (bb21a9b) - Clarence Manuel

- - -

## omni_cli_core-v0.9.0 - 2026-02-16
#### Features
- (**omni_tera**) add more inflection tera filterss - (d306802) - Clarence Manuel
#### Bug Fixes
- only ask to save generator session if there is something to save - (c03f6cc) - Clarence Manuel

- - -

## omni_cli_core-v0.8.0 - 2026-02-15
#### Features
- add args to run-generator action - (d8e9697) - Clarence Manuel
#### Bug Fixes
- (**omni_generator**) generators duplicate discovery - (9eecf57) - Clarence Manuel
- add support to template expansion in append-* prepend-* modify-* actions - (08faa16) - Clarence Manuel

- - -

## omni_cli_core-v0.7.0 - 2026-02-15
#### Features
- add task args - (199f686) - Clarence Manuel

- - -

## omni_cli_core-v0.6.3 - 2026-02-14
#### Bug Fixes
- task meta not discovered - (ec2412b) - Clarence Manuel

- - -

## omni_cli_core-v0.6.2 - 2026-02-14
#### Bug Fixes
- hide run-javascript action - (9cba894) - Clarence Manuel

- - -

## omni_cli_core-v0.6.1 - 2026-02-13
#### Bug Fixes
- optional value for --save-session and --ignore-session flag - (1284164) - Clarence Manuel
- add remember field for saveable prompts - (e4d4acd) - Clarence Manuel

- - -

## omni_cli_core-v0.6.0 - 2026-02-13
#### Features
- (**omni_tera**) add base_name and relative_path filters - (fcd4fe0) - Clarence Manuel
- support session namespacing for generator - (bcbfd04) - Clarence Manuel
- add support for toggle off template rendering - (e62d540) - Clarence Manuel
- support generator sessions - (7595efb) - Clarence Manuel
#### Bug Fixes
- (**bridge_rpc**) add stop signal to loop - (791ecf2) - Clarence Manuel
- (**omni_rpc**) base_name filter returning wrong text - (3e6c82a) - Clarence Manuel

- - -

## omni_cli_core-v0.5.4 - 2026-02-12
#### Bug Fixes
- support --inherit-env-vars flag in env subcommand - (e42f576) - Clarence Manuel
#### Miscellaneous Chores
- (**omni_remote_cache_client**) add timeout in tests - (85a30b2) - Clarence Manuel
- (**omni_remote_cache_client**) update crossplatform testing - (83fde88) - Clarence Manuel
- (**omni_remote_cache_client**) update test failure logs - (fdda3f3) - Clarence Manuel
- (**omni_remote_cache_client**) update test reliability - (f611fce) - Clarence Manuel

- - -

## omni_cli_core-v0.5.3 - 2026-02-10
#### Bug Fixes
- (**omni_core**) remove buggy extension graph linearization - (7d453fc) - Clarence Manuel
- (**omni_task_executor**) update verbose info trace to trace level - (84e9471) - Clarence Manuel
#### Miscellaneous Chores
- (**bridge_rpc**) increase timeout for integration test - (f5f8bc4) - Clarence Manuel
- (**omni_core**) update extension graph tests - (569c659) - Clarence Manuel
- update rust project presets - (3271200) - Clarence Manuel
- add traces - (58e5715) - Clarence Manuel

- - -

## omni_cli_core-v0.5.2 - 2026-02-09
#### Bug Fixes
- (**omni_task_executor**) expanding output paths for tera removes the OmniPath root - (15e3b82) - Clarence Manuel
#### Miscellaneous Chores
- (**omni_remote_cache_client**) improve omni binary path handling - (f49ae72) - Clarence Manuel
- update rust-multiplatform preset - (b3dba91) - Clarence Manuel
- update rust-multiplatform preset - (3ff7292) - Clarence Manuel

- - -

## omni_cli_core-v0.5.1 - 2026-02-08
#### Bug Fixes
- (**omni_cofigurations**) enable field default value - (db6ff61) - Clarence Manuel
#### Continuous Integration
- (**omni**) use rust-multiplaform preset - (cbe815e) - Clarence Manuel
#### Miscellaneous Chores
- improve rust-multiplatform preset - (4d46a14) - Clarence Manuel

- - -

## omni_cli_core-v0.5.0 - 2026-02-08
#### Features
- add meta to template context - (bd1c6d0) - Clarence Manuel
#### Bug Fixes
- (**omni_task_executor**) bug where persistent tasks causes panic on exit - (8de6991) - Clarence Manuel

- - -

## omni_cli_core-v0.4.0 - 2026-02-07
#### Features
- (**omni_configurations**) rename if to enabled in TaskLongFormConfiguration - (22df638) - Clarence Manuel
- (**omni_task_executor**) support output files and cache input key files in task result details - (212fe99) - Clarence Manuel
#### Bug Fixes
- (**omni_task_executor**) resolve workspace scoped paths - (45bb047) - Clarence Manuel
- (**omni_task_executor**) update task results details - (4daa71a) - Clarence Manuel
- rename fields - (efc8cb2) - Clarence Manuel
#### Miscellaneous Chores
- add target metadata to rust binary projects [skip ci] - (aeb9ea8) - Clarence Manuel

- - -

## omni_cli_core-v0.3.0 - 2026-02-04
#### Features
- (**serde_validate**) add Validated type [skip ci] - (8f25faf) - Clarence Manuel
- support tera template in cache input files and output files - (c0b305b) - Clarence Manuel
- support tera template in task command - (7830096) - Clarence Manuel
#### Bug Fixes
- improve error message when extended config is missing - (820c1ae) - Clarence Manuel

- - -

## omni_cli_core-v0.2.0 - 2026-02-03
#### Features
- support if expressions for task condition - (cb1c87d) - Clarence Manuel
#### Miscellaneous Chores
- (**bridge_rpc**) add max_retries - (92e5ee1) - Clarence Manuel

- - -

## omni_cli_core-v0.1.0 - 2026-01-31
#### Features
- (**bridge-rpc**) update implementation to v2 - (f7f38b8) - Clarence Manuel
- (**bridge_rpc**) implement receiving streams - (2004714) - Clarence Manuel
- (**bridge_rpc**) [WIP] implement streaming - (a28ab54) - Clarence Manuel
- (**bridge_rpc_router**) implement router - (20f0e20) - Clarence Manuel
- (**env**) implement command expansion - (69614a6) - Clarence Manuel
- (**omni_generator**) implement add-many action - (4156d24) - Clarence Manuel
- (**omni_generator_configurations**) utilize omni_serde_validators - (2dec237) - Clarence Manuel
- (**omni_generatore**) implement add-inline action - (61088ab) - Clarence Manuel
- (**omni_generatore**) add execution_actions stub - (f15d44f) - Clarence Manuel
- (**omni_generators**) implement add action - (9e9766f) - Clarence Manuel
- (**omni_generators**) add-inline implement prompting for missing target and expanding output path - (52ea1fb) - Clarence Manuel
- (**omni_path_utils**) fix doc test - (e796daf) - Clarence Manuel
- (**omni_prompt**) expand error_message in validators - (8b0bd99) - Clarence Manuel
- (**omni_prompt**) validate duplicate generator names - (995022f) - Clarence Manuel
- (**omni_prompt**) fix default value in template to support raw values - (d30952b) - Clarence Manuel
- (**omni_prompt**) support templates in default values - (292d337) - Clarence Manuel
- (**omni_prompt**) improve error handling in validator - (26d445c) - Clarence Manuel
- (**omni_prompt**) utilize requestty builtin validation - (69246a7) - Clarence Manuel
- (**omni_prompt**) utilize pre_exec_values - (723857b) - Clarence Manuel
- (**omni_prompt**) utilize prompt default values - (1e66706) - Clarence Manuel
- (**omni_prompt**) add support for customizing prompting config - (7321a8b) - Clarence Manuel
- (**omni_prompt**) add support for if condition and value validation - (00bb600) - Clarence Manuel
- (**omni_remote_cache_client**) add negative test - (7561e0f) - Clarence Manuel
- (**omni_remote_cache_client**) add tests - (9917ded) - Clarence Manuel
- (**omni_remote_cache_service**) implement basic functionality - (bbb9f62) - Clarence Manuel
- (**omni_remote_cache_service_client**) add security headers to default implementation [skip ci] - (662656a) - Clarence Manuel
- (**omni_remote_cache_service_client**) add default implementation for RemoteCacheServiceClient - (1aa19ba) - Clarence Manuel
- (**omni_remote_cache_service_client**) add RemoteCacheServiceClient trait - (a45a3a8) - Clarence Manuel
- (**omni_serde_validators**) extract common validators into one crate - (f9657cb) - Clarence Manuel
- (**omni_task_executor**) utilize omni_tracing_subscriber ad-hoc writer for TUI mode - (413a3f1) - Clarence Manuel
- (**omni_task_executor**) add batch_executor module - (dfe27a2) - Clarence Manuel
- (**omni_task_executor**) add cache_manager module - (310e4b2) - Clarence Manuel
- (**omni_task_executor**) add task_context_provider module - (2db2b7d) - Clarence Manuel
- (**omni_term_ui**) add scrollbar to term ui - (c51c1ae) - Clarence Manuel
- (**omni_term_ui**) support cursor movements - (f81a428) - Clarence Manuel
- (**omni_term_ui**) implement key event handling in TUI mode - (123ccb0) - Clarence Manuel
- (**omni_term_ui**) implement scrolling on TUI - (ec11707) - Clarence Manuel
- (**omni_tracing_subscriber**) support for custom ad-hoc writers - (f584f1f) - Clarence Manuel
- (**serde_validate**) implement generic tuple implementations for validators - (f80004c) - Clarence Manuel
- [WIP] update implementation - (5b267cd) - Clarence Manuel
- [WIP] add server response - (5db995e) - Clarence Manuel
- [WIP] add server request - (f64a539) - Clarence Manuel
- [WIP] rust bridge-v2 implementation - (1e69b9a) - Clarence Manuel
- initial implementation of run-javascript - (dd9d040) - Clarence Manuel
- implement run-command action - (a1bf24f) - Clarence Manuel
- add output_path on template context - (2b950af) - Clarence Manuel
- add data support to modify, modify-content, append, append-content, prepend, prepend-content - (a80ff07) - Clarence Manuel
- implement append, append-content, prepend, prepend-content actions - (3260ed1) - Clarence Manuel
- implement modify and modify-content actions - (edfd7c4) - Clarence Manuel
- support target override on run-generator action - (a63f18c) - Clarence Manuel
- support data context to add-* actions - (8dab0c8) - Clarence Manuel
- allow arbitrary value for vars in generator.omni.yaml - (f32d77f) - Clarence Manuel
- allow overriding targets in CLI args - (b5aff09) - Clarence Manuel
- implement run-generator action - (9cb9f7c) - Clarence Manuel
- implement generator list command - (5bfc5c7) - Clarence Manuel
- update omni_configurations dependencies - (89758a1) - Clarence Manuel
- support negative patterns in discovering config files - (1b52131) - Clarence Manuel
- add --overwrite flag to gen run command - (5cda5d7) - Clarence Manuel
- implement strip_extensions - (36a9258) - Clarence Manuel
- add log for retry interval - (268912e) - Clarence Manuel
- implement retry interval - (209a039) - Clarence Manuel
- implement task retry - (9b5232b) - Clarence Manuel
- utilize omni_generator crate and implement basic prompting - (d835d3c) - Clarence Manuel
- trim value processd by validate_value and skip - (d76ebc2) - Clarence Manuel
- implement gen run command handling - (b9856c9) - Clarence Manuel
- add parser for answers flag - (a463afa) - Clarence Manuel
- add stub commands for generator - (55cbc8c) - Clarence Manuel
- implement omni_prompt crate - (48ab0df) - Clarence Manuel
- add Template type [skip ci] - (b85a5b8) - Clarence Manuel
- modify generator configuration - (e887b4f) - Clarence Manuel
- add generate subcommand - (9206627) - Clarence Manuel
- implement --with-dependents - (f0421a4) - Clarence Manuel
- implement --scm-affected flag - (5bf7c28) - Clarence Manuel
- implement --dir filter - (30449d6) - Clarence Manuel
- secure storage for remote cache config - (9d9317b) - Clarence Manuel
- add support for multiple flags for --project and --task - (6b4532c) - Clarence Manuel
- add initial omni_generator_configurations crate implementation - (f80884b) - Clarence Manuel
- implement --force=failed flag - (b3a04b8) - Clarence Manuel
- add hash project subcommand - (40f5d1a) - Clarence Manuel
- update path for remote caching configuration - (a911520) - Clarence Manuel
- implement setup command for remote cache - (e2ae4d0) - Clarence Manuel
- implement head /artifacts for credentials validation - (5772be3) - Clarence Manuel
- implement loading of remote-cache config - (09b6361) - Clarence Manuel
- utilize RemoteCacheClient in omni_cache - (5d04312) - Clarence Manuel
- implement experimental --stale-only flag - (1d6029b) - Clarence Manuel
- implement cache prune command - (1726796) - Clarence Manuel
- implement cache stats command - (265706c) - Clarence Manuel
- add cache stats and prune mock commands - (6952963) - Clarence Manuel
- refactor declspec subcommand - (cef37ee) - Clarence Manuel
- experimental tui mode - (a44bbad) - Clarence Manuel
- add omni_term_ui stream implementation - (57505b5) - Clarence Manuel
- add cache dir subcommand - (e3abdd9) - Clarence Manuel
- allow env:vars in workspace configuration - (c2b03e0) - Clarence Manuel
- utilize command expansion - (04d38d1) - Clarence Manuel
- rename print-schema to schema - (75eaa82) - Clarence Manuel
- add more info to declspec - (d55ea1d) - Clarence Manuel
- declspec command - (a8e23a8) - Clarence Manuel
- support --result flag for exec and run command - (3311ba8) - Clarence Manuel
- add --dry-run flag to exec and run command - (e458657) - Clarence Manuel
- support sibling task, disabling task - (2f9ff3a) - Clarence Manuel
- add max_concurrency flag - (13b4d39) - Clarence Manuel
- add raw_value flag to hash command - (5d07d11) - Clarence Manuel
- return appropriate exit code on exec and run command, add --no-replay-logs flag, add hash workspace command - (0106e12) - Clarence Manuel
- improve load_projects performance by 50-60% - (869b91e) - Clarence Manuel
- convert load_projects to async - (23e26c3) - Clarence Manuel
- support empty commands - (211b260) - Clarence Manuel
- improve load_projects performance from 20-50% by utilizing build_parallel in ignore - (5a55399) - Clarence Manuel
- remove unnecessary canonicalize - (3b45989) - Clarence Manuel
- improve load_projects performance by 50% - (94f2084) - Clarence Manuel
- add omni load_projects benchmarks - (d93c8ea) - Clarence Manuel
- add omni_test_utils - (941e1bd) - Clarence Manuel
- support for meta filter - (238661e) - Clarence Manuel
- add omni_expressions - (31fa618) - Clarence Manuel
- added execution time on cache outputs - (c4025d9) - Clarence Manuel
- color reporting and resolve @project to the current project's - (4d3dc8e) - Clarence Manuel
- record and replay logs - (1f33d00) - Clarence Manuel
- enhance topmost_dir to support disjoint paths - (c257f14) - Clarence Manuel
- integrate caching to TaskOrchestrator - (5e98052) - Clarence Manuel
- new on_failure handling - (2f42dc9) - Clarence Manuel
- add exit_code and execution_time - (bf92f79) - Clarence Manuel
- load cache_keys in load_projects - (5c8ac18) - Clarence Manuel
- add test for invalidate_caches - (775e191) - Clarence Manuel
- support get_many and cache_many operation - (a408ccd) - Clarence Manuel
- support @workspace in input and output files - (e1edb84) - Clarence Manuel
- support OmniPath in caching - (9ae0c25) - Clarence Manuel
- LocalTaskExecutionCacheStore initial implementation - (36758a6) - Clarence Manuel
- RealDirHashBuilder implementation + tests - (6f01e4b) - Clarence Manuel
- utilize OmniPath for other path configurations - (5d8deff) - Clarence Manuel
- utilize portable-pty - (b691f3d) - Clarence Manuel
- utilize TaskExecutor - (3ca5224) - Clarence Manuel
- utilize OmniPath syntax for path configurations - (ce111ed) - Clarence Manuel
- support expansions on task env vars - (dc717b7) - Clarence Manuel
- support env values per task - (1663fa2) - Clarence Manuel
- utilize maps and sets create in the CLI - (5c55f01) - Clarence Manuel
- improve env parser performance by 7% - (14d1fd7) - Clarence Manuel
- improve env parser performance by 99% - (0528a88) - Clarence Manuel
- global maps and sets in config_utils, env, and env_loader - (835b9fd) - Clarence Manuel
- create crates for global maps and sets implementation - (da41d3e) - Clarence Manuel
- update config_utils to replace discriminant - (c2f6a62) - Clarence Manuel
- tracing with file - (44e2d31) - Clarence Manuel
- add description column for task - (f6121b4) - Clarence Manuel
- unit test for expand_into - (d717ce5) - Clarence Manuel
- add support for env overrides expansion - (0bbbed9) - Clarence Manuel
- custom env_files + override - (cb095b7) - Clarence Manuel
- improve process_node_by_id cache retrieval - (03457a9) - Clarence Manuel
- add WORKSPACE_DIR and PROJECT_DIR support on extend paths - (3ed4c6a) - Clarence Manuel
- integrate ExtensionGraph to Context::load_projects - (87dc968) - Clarence Manuel
- extension_graph passing tests - (22af63e) - Clarence Manuel
- add merge support for configurations - (b952e72) - Clarence Manuel
- support deep merge in config_utils - (84d2d99) - Clarence Manuel
- support more characters in task dependency syntax - (39ed73e) - Clarence Manuel
- add BridgeRpc integration tests - (5710a86) - Clarence Manuel
- add StreamTransport to bridge_rpc crate - (0047182) - Clarence Manuel
- add TransportReadFramer and TransportWriteFramer to bridge_rpc crate - (85faf60) - Clarence Manuel
- use bytes crate for bridge_rpc - (64403db) - Clarence Manuel
- add probe to bridge-rpc-ts - (118bb26) - Clarence Manuel
- add probe test in bridge_rpc crate - (f5750e7) - Clarence Manuel
- add close_ack, probe, probe_ack support for bridge_rpc crate - (704cf54) - Clarence Manuel
- add typescript projects - (028e56c) - Clarence Manuel
- ignore run dependencies and failures - (d0ef6da) - Clarence Manuel
- bridge_rpc tests - (c88b1d6) - Clarence Manuel
- WIP bridge_rpc - (6e6a556) - Clarence Manuel
- utilize BatchedExecutionPlan in run command - (8d754ac) - Clarence Manuel
- serializable ProjectGraph and TaskExecutionGraph - (6c2e647) - Clarence Manuel
- create get_project_graph method - (d098153) - Clarence Manuel
- update testing to support batched_execution_plan - (0ca4da5) - Clarence Manuel
- batched execution plan - (0dc6263) - Clarence Manuel
- test get_task_execution_graph - (f5b6ca0) - Clarence Manuel
- test own and explicit dependency handling - (dde35ee) - Clarence Manuel
- test upstream dependencies - (fe548cb) - Clarence Manuel
- test from_project_graph - (7976665) - Clarence Manuel
- add from_projects test - (ee35ce5) - Clarence Manuel
- project graph - (a3fcb4d) - Clarence Manuel
- js_runtime configuration - (56752bd) - Clarence Manuel
- add test for load_projects duplicate project name scenario - (4724706) - Clarence Manuel
- add test for load_projects - (1720882) - Clarence Manuel
- use deno_task_shell - (c0aa5d4) - Clarence Manuel
- WIP DirWalker trait - (fda06d1) - Clarence Manuel
- add tests to env_loader - (7f46f4c) - Clarence Manuel
- WIP JsRuntime - (04270a6) - Clarence Manuel
- implemented filter - (0aa1e69) - Clarence Manuel
- created run command - (d2e898d) - Clarence Manuel
- env parser - (dbdddef) - Clarence Manuel
#### Bug Fixes
- (**bridge_rpc**) test for duplicate paths in builder - (691ae42) - Clarence Manuel
- (**bridge_rpc**) test_stream_data integration test - (8b86fdf) - Clarence Manuel
- (**bridge_rpc**) unit tests - (0920ac7) - Clarence Manuel
- (**bridge_rpc**) create_stream_handler signature to allow start_data - (bba9b6d) - Clarence Manuel
- (**bridge_rpc**) failing tests due to rmp not serializing names - (e84f55d) - Clarence Manuel
- (**omni_cache**) fix unnecessary override in EnvLoader [skip ci] - (12703e8) - Clarence Manuel
- (**omni_cli_core**) broken log colors due to tracing-subscriber 0.3.20 - (91ceb27) - Clarence Manuel
- (**omni_cli_core**) fix failing tests - (996174f) - Clarence Manuel
- (**omni_cli_core**) compile error - (706db83) - Clarence Manuel
- (**omni_generator_configurations**) remove serde flatten on unused fields - (1dc1f8e) - Clarence Manuel
- (**omni_generators**) fix failing test on windows - (28f95f3) - Clarence Manuel
- (**omni_process**) pty mode slow logs - (7e74671) - Clarence Manuel
- (**omni_prompt**) values passed as pre exec are the wrong type - (50a5dc0) - Clarence Manuel
- (**omni_serde_validators**) fix dependency name - (d48a097) - Clarence Manuel
- (**omni_task_executor**) fix Cargo.toml - (75e5480) - Clarence Manuel
- (**omni_task_executor**) no cache hits bug - (a0f2b01) - Clarence Manuel
- (**omni_task_executor**) compile error - (0eb44cf) - Clarence Manuel
- (**omni_term_ui**) use tickrate and enhance responsiveness of TUI - (9f9ae61) - Clarence Manuel
- (**omni_term_ui**) implement follow trail autoscroll - (e630f23) - Clarence Manuel
- (**omni_term_ui**) add color handling to tui mode - (96d0f5a) - Clarence Manuel
- no expansion of command and env vars - (66b84e0) - Clarence Manuel
- generator panic due to non existing arg - (43a95f4) - Clarence Manuel
- rename --out-dir to --output - (c73bee3) - Clarence Manuel
- data properties not being expanded - (ddcd2f1) - Clarence Manuel
- cycle for project config - (04f5556) - Clarence Manuel
- support flatten option for add and add-many actions - (5299115) - Clarence Manuel
- base_path not taking effect on add-many - (1f7355a) - Clarence Manuel
- retry flag precedence - (b60924f) - Clarence Manuel
- panic when running exec - (19ce74f) - Clarence Manuel
- task_configuration.retry_interval json schema and serialization - (5d6df02) - Clarence Manuel
- invert precedence of --retry and --retry-interval precedence for CLI and file config - (1866d88) - Clarence Manuel
- validate_value logic reversed - (ae420d2) - Clarence Manuel
- rename checkbox prompt to confirm prompt - (00aa84e) - Clarence Manuel
- prompt configuration schema - (4f80238) - Clarence Manuel
- typo in dependency causing CI failure - (64e57a2) - Clarence Manuel
- typo in project name causing CI failure - (ded70ab) - Clarence Manuel
- typo in dependency causing CI failure - (619cc57) - Clarence Manuel
- dependency cycle - (bc97e37) - Clarence Manuel
- macos compile error - (bb8f4d1) - Clarence Manuel
- non deterministic ordering of project loading causing no deterministic extension graph - (95ec8c0) - Clarence Manuel
- wrong extension order for extension_graph, use Bfs instead of Dfs - (48be31d) - Clarence Manuel
- rename siblings field to with in project_configuration - (48fd996) - Clarence Manuel
- no hash when running dry run - (f4495c9) - Clarence Manuel
- integer overflow - (f75a162) - Clarence Manuel
- default values for CacheConfiguration and TaskConfiguration - (ac8329f) - Clarence Manuel
- merging behavior for CacheConfiguration - (43c3313) - Clarence Manuel
- output globs should influence the task execution digest - (31c136f) - Clarence Manuel
- default value for persistent and interactive form TaskConfigurationLongForm - (bd8bd28) - Clarence Manuel
- merging behavior for enabled, persistent, interactive in TaskConfigurationLongForm - (ce1a976) - Clarence Manuel
- term ui scrolling not working - (7284f62) - Clarence Manuel
- path for omni file trace logs is relative to the current dir, should be relative to workspace root dir - (1996de0) - Clarence Manuel
- rephrased prompt for prune confirmation - (261b1d6) - Clarence Manuel
- add warning for using --stale-only - (68b05ca) - Clarence Manuel
- cached file paths error when running cache stats and cache prune - (eb0da1d) - Clarence Manuel
- not skipping task when there are dependency failures - (c5348cc) - Clarence Manuel
- env:files config should only be available in workspace config - (7111d3e) - Clarence Manuel
- CacheKeyConfiguration defaults should only be replaced if right value is Some - (d86b6dd) - Clarence Manuel
- exec command exiting early due to output writer being dropped early - (3e98a7d) - Clarence Manuel
- no logs output until child_process is finished - (3e94250) - Clarence Manuel
- exit 1 due to improper handling if completed but non zero logic - (69147f2) - Clarence Manuel
- stderr trace flag inverted logic - (bd40deb) - Clarence Manuel
- traces causing invalid print-schema output - (d854440) - Clarence Manuel
- no stderr log output non pty sessions - (09c617a) - Clarence Manuel
- compilation error on windows - (75f9a36) - Clarence Manuel
- rename is_skipped_or_error to is_failure - (856edb2) - Clarence Manuel
- exit_code non 0 should be considered error - (6e254f0) - Clarence Manuel
- remove invalid negative flags - (184813f) - Clarence Manuel
- restoring cache output causing error when dir doesn't exist - (f6ddf24) - Clarence Manuel
- context should not be loaded for commands that don't need it - (f1c9f86) - Clarence Manuel
- fix hash workspace command not hashing deterministically - (44050e9) - Clarence Manuel
- add test for persistent task - (6118f72) - Clarence Manuel
- wrong exit code when a task is skipped due to being disabled - (b6496f6) - Clarence Manuel
- unit test for persistent task - (ced08df) - Clarence Manuel
- bug where persistent task are always executed - (c720129) - Clarence Manuel
- persistent task should not be cached and it should keep stdin open - (416b76e) - Clarence Manuel
- performance issue due to include override ignoring ignored files - (88944fa) - Clarence Manuel
- flaky context tests - (759f4dc) - Clarence Manuel
- increase default concurrent tasks - (8633e0d) - Clarence Manuel
- build error on windows - (741cb8a) - Clarence Manuel
- build error on unix - (155716a) - Clarence Manuel
- handle non pty compatible scenarios - (9697e2f) - Clarence Manuel
- windows not loading projects - (a12bc9e) - Clarence Manuel
- test_fixtures on env_loader - (68aeafe) - Clarence Manuel
- don't use as_bytes on &OsStr as it is unix specific - (6a9d66d) - Clarence Manuel
- add feature gate to prevent windows build from failing - (acddf9b) - Clarence Manuel
- add feature gate to prevent windows build from failing - (09f9a2f) - Clarence Manuel
- duplicate loaded files due to concurrent inserts - (4f12e4a) - Clarence Manuel
- use u32 instead of i32 for exit_code - (ab5762d) - Clarence Manuel
- bug in exit code handling when cached - (6d908b1) - Clarence Manuel
- default trace level - (274eaf5) - Clarence Manuel
- tests ignore files not working properly - (f12cd8a) - Clarence Manuel
- unit tests - (b9b7300) - Clarence Manuel
- cached output files linking and *ignore file handling - (f1553ce) - Clarence Manuel
- exec command - (13734da) - Clarence Manuel
- test_cached_file_content test - (e85a0f2) - Clarence Manuel
- compile error when without enable-tracing - (950b2af) - Clarence Manuel
- log implementation and project task - (7c7b768) - Clarence Manuel
- bridge_rpc crate probe bug - (a70964a) - Clarence Manuel
- performance issue due to incorrect ignore files configuration - (db49db7) - Clarence Manuel
- update env test data - (ace15ff) - Clarence Manuel
- test data - (6b5d956) - Clarence Manuel
- compile speed on dev - (69b4609) - Clarence Manuel
#### Performance Improvements
- (**env_loader**) reduce map cloning by using arc - (4043962) - Clarence Manuel
- add omni_context benchmarks - (55628d7) - Clarence Manuel
#### Continuous Integration
- fix error - (759d8d5) - Clarence Manuel
#### Refactoring
- (**bridge_rpc**) cleanup code - (ab76ce4) - Clarence Manuel
- (**bridge_rpc**) stream implementation - (28bebff) - Clarence Manuel
- (**bridge_rpc**) cleanup Frame implementation - (962f77f) - Clarence Manuel
- (**bridge_rpc**) use tokio traits instead of futures traits - (9db748d) - Clarence Manuel
- (**clap_utils**) create ValueEnumAdapter - (e0fc922) - Clarence Manuel
- (**omni_configuration_discovery**) extract crates with duplicated functionality - (f6f530d) - Clarence Manuel
- (**omni_context**) clean up project_discovery [skip ci] - (5bff426) - Clarence Manuel
- (**omni_context**) cleanup code [skip ci] - (680741e) - Clarence Manuel
- (**omni_context**) re enable tests - (e9ee414) - Clarence Manuel
- (**omni_context**) utilize ProjectQuery - (836fc52) - Clarence Manuel
- (**omni_execution_plan**) create reusable omni_execution_plan crate - (c583161) - Clarence Manuel
- (**omni_generator**) cleanup add-inline, add, and add-many handlers - (d65be3e) - Clarence Manuel
- (**omni_prompt**) cleanup - (25115b2) - Clarence Manuel
- (**omni_prompt**) cleanup code [skip ci] - (3849ceb) - Clarence Manuel
- (**omni_prompt**) cleanup code [skip ci] - (6e61710) - Clarence Manuel
- (**omni_task_context**) create reusable omni_task_context crate - (f154cf4) - Clarence Manuel
- (**omni_task_executor**) add result, error, sys types - (4a547a0) - Clarence Manuel
- (**omni_test_utils**) remove omni_cli_core as dependency and use omni_configurations instead - (1e7fcfb) - Clarence Manuel
- remove serialization logic in response and request types - (32c53c2) - Clarence Manuel
- separate client and server request and response types - (76ff9dd) - Clarence Manuel
- microoptimization - (45ed0fa) - Clarence Manuel
- error types - (2c0dd4c) - Clarence Manuel
- rename types - (ed5ea96) - Clarence Manuel
- cleanup code - (718d78f) - Clarence Manuel
- extract run_custom_commons module for run_command and run_javascript handler - (2da659f) - Clarence Manuel
- update configuration discovery generic signature - (44d0c37) - Clarence Manuel
- cleanup code - (e1db62c) - Clarence Manuel
- generator run and run_internal parameters - (f2ab618) - Clarence Manuel
- vars expansion - (6cf6f5d) - Clarence Manuel
- upgrade OmniPath implementation to allow different roots - (e59749e) - Clarence Manuel
- microoptimization - (026105b) - Clarence Manuel
- make static variables for file names - (7a6e41c) - Clarence Manuel
- update get_output_path signature - (ed0cd89) - Clarence Manuel
- cleanup code - (e985af9) - Clarence Manuel
- rename error result status to errored - (adad5d7) - Clarence Manuel
- exec command args - (22f9d6b) - Clarence Manuel
- omni presets and logs - (13dc57c) - Clarence Manuel
- rename hash fields to digest - (90ad6c9) - Clarence Manuel
- cleanup omni_context [skip ci] - (7c94419) - Clarence Manuel
- major refactor for omni_context and related crates - (28f69e7) - Clarence Manuel
- start creating omni_task_executor crate [skip ci] - (5e4899e) - Clarence Manuel
- create omni_context crate [skip ci] - (4fefab0) - Clarence Manuel
- create omni_configurations crate - (4b3a7b1) - Clarence Manuel
- rename ScriptingConfiguration to ExecutorsConfiguration - (cb3ae35) - Clarence Manuel
- child_process [skip ci] - (370174f) - Clarence Manuel
- fix clippy warning [skip ci] - (2a3ddde) - Clarence Manuel
- TaskExecutionResult type [skip ci] - (9c2714c) - Clarence Manuel
- common args in run and exec - (347c2c4) - Clarence Manuel
- omni_process - (7fa0852) - Clarence Manuel
- move all collect features to omni_collector crate - (17bb79e) - Clarence Manuel
- utilize refactored omni_collecter, omni_utils, omni_hasher in omni_cache - (a995400) - Clarence Manuel
- refactor ProjectDirHasher from omni_cache to omni_hasher - (c9e70c7) - Clarence Manuel
- omni_utils from omni_cache - (55e8e00) - Clarence Manuel
- omni_collector refactored from omni_cache - (adf6cdb) - Clarence Manuel
- create TaskOrchestrator to generalize run and exec commands - (8af7849) - Clarence Manuel
- TaskExecutionGraph to avoid returning error with TaskKey details - (069d27e) - Clarence Manuel
- omni_types WsPath - (6e09b61) - Clarence Manuel
- error types to use strum - (0076af9) - Clarence Manuel
- pattern matching - (a711a21) - Clarence Manuel
- core types - (ffa5294) - Clarence Manuel
#### Miscellaneous Chores
- (**bridge_rpc**) add response integration test - (869b3bb) - Clarence Manuel
- (**omni_remote_cache_client**) update tests - (542b0f5) - Clarence Manuel
- set versions and update cog.toml [skip ci] - (c6efdcf) - Clarence Manuel
- update crates [skip ci] - (8b9022a) - Clarence Manuel
- bump omni version - (d35b973) - Clarence Manuel
- update Cargo.lock and Cargo.toml - (4f42230) - Clarence Manuel
- add trace for prompt values - (a847152) - Clarence Manuel
- add test for flatten option - (a508c10) - Clarence Manuel
- remove experimental warning for --stale-only - (7383a69) - Clarence Manuel
- update doc comments - (74e3a88) - Clarence Manuel
- rename properties - (e3a3933) - Clarence Manuel
- add doc comments - (654f0eb) - Clarence Manuel
- update error implementations - (ad81dc0) - Clarence Manuel
- fix typo [skip ci] - (362ae18) - Clarence Manuel
- update dependencies [skip ci] - (5c6db83) - Clarence Manuel
- add traces - (e8aebf6) - Clarence Manuel
- env project add maps dependency - (f1b52cc) - Clarence Manuel
- env project profile task description - (21d1b60) - Clarence Manuel
- upgrade dependencies - (766d147) - Clarence Manuel
- update command documentations - (ff28183) - Clarence Manuel
- update dependencies [skip ci] - (280976f) - Clarence Manuel
- add traces - (8077e93) - Clarence Manuel
- update dependencies [skip ci] - (d31a4ea) - Clarence Manuel
- cleanup code [skip ci] - (c7f4c98) - Clarence Manuel
- cleanup code [skip ci] - (fcc1458) - Clarence Manuel
- cleanup unneeded files [skip ci] - (04d1c09) - Clarence Manuel
- fix clippy warnings [skip ci] - (d64926c) - Clarence Manuel
- add publish-json-schemas workflow [skip ci] - (8144c86) - Clarence Manuel
- update yaml-language-server schema on omni config files [skip ci] - (554ea4d) - Clarence Manuel
- update omni Cargo.toml to use own version - (758716d) - Clarence Manuel
- context tests - (f997e5a) - Clarence Manuel
- refactor omni_type tests - (5cd2ca5) - Clarence Manuel
- add windows case on omni_types OmniPath test cases - (381c361) - Clarence Manuel
- update Cargo project manifests - (7f5fdc3) - Clarence Manuel
- add omni_generator project - (dc529ef) - Clarence Manuel
- improve benchmakrs - (c7efa55) - Clarence Manuel
- clean up - (e5a7466) - Clarence Manuel
- update test to better reflect real world scenario - (0a13295) - Clarence Manuel
- sync changes - (6c9a887) - Clarence Manuel
- update configs - (01f02c5) - Clarence Manuel
- sync changes - (96d6b2f) - Clarence Manuel
- refactore tests - (7b072b4) - Clarence Manuel
- sync changes - (7e7daae) - Clarence Manuel
- add test for deserializing with special characters - (0abe015) - Clarence Manuel
- sync updates - (c494e10) - Clarence Manuel
- sync - (1c70cef) - Clarence Manuel
- update about - (490b135) - Clarence Manuel
- add deno - (75903c5) - Clarence Manuel
- initial commit - (4db3eaf) - Clarence Manuel

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).