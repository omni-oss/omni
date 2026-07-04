# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## omni_generator-v0.10.0 - 2026-07-03
#### Features
- upgrade tera version and update presets - (c097dc8) - Clarence Manuel
- remove default_expr and support Object input - (8f7e84a) - Clarence Manuel
#### Bug Fixes
- (**omni_generator**) utilize new generator events and convert info logs to debug logs - (5d588ce) - Clarence Manuel
- (**omni_generator**) TransactionSys info logs should be debug - (20dc6dc) - Clarence Manuel
- rename FILENAME to FILEPATH environmental variable in transform-many action - (7f7eb0c) - Clarence Manuel
- use progress indicators for generator progress - (2022354) - Clarence Manuel
- swap serde_norway for noyalib and improve serialization-related error reporting - (7c65947) - Clarence Manuel
- rename and refactoring of input mechanism for generator - (cb8fa22) - Clarence Manuel
#### Refactoring
- (**omni_generator**) rename utility methods - (8def5d5) - Clarence Manuel
- create builder extensions - (635d671) - Clarence Manuel
- use builder for creating inputs - (e59d909) - Clarence Manuel

- - -

## omni_generator-v0.9.0 - 2026-06-21
#### Features
- (**omni_generator**) implement transform and transform-many actions - (f14e39f) - Clarence Manuel
- add initial mcp support - (a5c8b6e) - Clarence Manuel
- implement run-javascript action - (7800281) - Clarence Manuel
#### Bug Fixes
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
- expose description field in generator inputs - (46bc836) - Clarence Manuel
- rename prompts to inputs - (516dba6) - Clarence Manuel
#### Refactoring
- (**omni_generator**) make GenSession methods async - (26640a8) - Clarence Manuel
- introduce and use input builders - (25221a0) - Clarence Manuel
- allow optionally passing arbitrary version file to bridge service vendoring - (56d3912) - Clarence Manuel
- expose base method for ActionConfiguration - (58decdc) - Clarence Manuel
- create a central omni_api and omni_messages crate extracted from omni_cli_core - (eba8ec1) - Clarence Manuel
- improve omni_generator architecture and testability - (487a134) - Clarence Manuel
#### Miscellaneous Chores
- (**omni_generator**) update DryRunSys implementation - (dc4486a) - Clarence Manuel

- - -

## omni_generator-v0.8.0 - 2026-05-22
#### Features
- add and utilize standard values for tera context - (92c8e38) - Clarence Manuel

- - -

## omni_generator-v0.7.0 - 2026-05-18
#### Features
- support remote git generator sources - (c83d835) - Clarence Manuel
- update init command to require primary generator config - (a035443) - Clarence Manuel
- implement init subcommand - (ba0e005) - Clarence Manuel
#### Bug Fixes
- tera error due to name mismatch - (b004de8) - Clarence Manuel
#### Refactoring
- use log for user facing logs - (4ddf7c5) - Clarence Manuel
#### Miscellaneous Chores
- update omni configs json schema links [skip ci] - (d484be7) - Clarence Manuel

- - -

## omni_generator-v0.6.0 - 2026-03-21
#### Features
- implement --use-defaults flag for generator - (57e8255) - Clarence Manuel

- - -

## omni_generator-v0.5.0 - 2026-03-14
#### Features
- support overriding output_dir on run_generator actions - (c5524a7) - Clarence Manuel

- - -

## omni_generator-v0.4.0 - 2026-02-25
#### Features
- support multiple entries for modify-* actions - (196c184) - Clarence Manuel
- support multiple entries for prepend and append actions - (039388d) - Clarence Manuel

- - -

## omni_generator-v0.3.2 - 2026-02-17
#### Bug Fixes
- (**omni_generator**) dry run write error - (fa3d46f) - Clarence Manuel

- - -

## omni_generator-v0.3.1 - 2026-02-16
#### Bug Fixes
- only ask to save generator session if there is something to save - (c03f6cc) - Clarence Manuel

- - -

## omni_generator-v0.3.0 - 2026-02-15
#### Features
- add args to run-generator action - (d8e9697) - Clarence Manuel
#### Bug Fixes
- (**omni_generator**) generators duplicate discovery - (9eecf57) - Clarence Manuel
- add support to template expansion in append-* prepend-* modify-* actions - (08faa16) - Clarence Manuel

- - -

## omni_generator-v0.2.1 - 2026-02-13
#### Bug Fixes
- add remember field for saveable prompts - (e4d4acd) - Clarence Manuel

- - -

## omni_generator-v0.2.0 - 2026-02-13
#### Features
- support session namespacing for generator - (bcbfd04) - Clarence Manuel
- add support for toggle off template rendering - (e62d540) - Clarence Manuel
- support generator sessions - (7595efb) - Clarence Manuel

- - -

## omni_generator-v0.1.0 - 2026-01-31
#### Features
- (**omni_generator**) implement add-many action - (4156d24) - Clarence Manuel
- (**omni_generatore**) implement add-inline action - (61088ab) - Clarence Manuel
- (**omni_generatore**) add execution_actions stub - (f15d44f) - Clarence Manuel
- (**omni_generators**) implement add action - (9e9766f) - Clarence Manuel
- (**omni_generators**) add-inline implement prompting for missing target and expanding output path - (52ea1fb) - Clarence Manuel
- (**omni_prompt**) validate duplicate generator names - (995022f) - Clarence Manuel
- (**omni_tracing_subscriber**) support for custom ad-hoc writers - (f584f1f) - Clarence Manuel
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
- add --overwrite flag to gen run command - (5cda5d7) - Clarence Manuel
- implement strip_extensions - (36a9258) - Clarence Manuel
- utilize omni_generator crate and implement basic prompting - (d835d3c) - Clarence Manuel
- implement omni_prompt crate - (48ab0df) - Clarence Manuel
- add Template type [skip ci] - (b85a5b8) - Clarence Manuel
#### Bug Fixes
- (**omni_generators**) fix failing test on windows - (28f95f3) - Clarence Manuel
- no expansion of command and env vars - (66b84e0) - Clarence Manuel
- data properties not being expanded - (ddcd2f1) - Clarence Manuel
- support flatten option for add and add-many actions - (5299115) - Clarence Manuel
- base_path not taking effect on add-many - (1f7355a) - Clarence Manuel
- prompt configuration schema - (4f80238) - Clarence Manuel
- typo in dependency causing CI failure - (64e57a2) - Clarence Manuel
- typo in project name causing CI failure - (ded70ab) - Clarence Manuel
- typo in dependency causing CI failure - (619cc57) - Clarence Manuel
#### Refactoring
- (**omni_generator**) cleanup add-inline, add, and add-many handlers - (d65be3e) - Clarence Manuel
- microoptimization - (45ed0fa) - Clarence Manuel
- cleanup code - (718d78f) - Clarence Manuel
- extract run_custom_commons module for run_command and run_javascript handler - (2da659f) - Clarence Manuel
- cleanup code - (e1db62c) - Clarence Manuel
- generator run and run_internal parameters - (f2ab618) - Clarence Manuel
- vars expansion - (6cf6f5d) - Clarence Manuel
- microoptimization - (026105b) - Clarence Manuel
- make static variables for file names - (7a6e41c) - Clarence Manuel
- update get_output_path signature - (ed0cd89) - Clarence Manuel
- cleanup code - (e985af9) - Clarence Manuel
#### Miscellaneous Chores
- set versions and update cog.toml [skip ci] - (c6efdcf) - Clarence Manuel
- add trace for prompt values - (a847152) - Clarence Manuel
- add test for flatten option - (a508c10) - Clarence Manuel
- update error implementations - (ad81dc0) - Clarence Manuel
- upgrade dependencies - (766d147) - Clarence Manuel
- add publish-json-schemas workflow [skip ci] - (8144c86) - Clarence Manuel
- update yaml-language-server schema on omni config files [skip ci] - (554ea4d) - Clarence Manuel
- add omni_generator project - (dc529ef) - Clarence Manuel

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).