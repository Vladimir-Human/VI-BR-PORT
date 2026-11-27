# What CI checks

One job on `windows-latest` (`.github/workflows/ci.yml`) runs on every pull
request and every push to `main`. Its steps, cheapest first, and how to run
each on your machine before you push:

| Step | Command | Time (warm cache) |
|---|---|---|
| Repository rules | `cargo test -p xtask --locked` | seconds |
| Clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | ~30 s |
| Clippy, diagnostics build | `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | ~25 s |
| Documentation | `cargo doc --workspace --no-deps --all-features --locked` with `RUSTDOCFLAGS="-D warnings -A rustdoc::private_intra_doc_links"` | ~20 s |
| Tests | `cargo test --workspace --locked` | ~2 min |

`--locked` everywhere also checks that `Cargo.lock` is up to date.

`cargo xtask rules` prints the repository rules' findings without the test
harness; `cargo xtask install-hooks` makes every commit run the asset policy.

## The repository rules

Each is a test in `xtask` that reads the sources as text (no build of the
crates it checks). A rule that needs an exception has a list with a reason
for each entry in the rule's own file; keep those lists short.

- **Asset policy** (`xtask/src/policy.rs`, `repository_passes_policy`): the
  repository holds only small text files we wrote; see
  [asset-policy.md](asset-policy.md).
- **Shader cache switches** (`xtask/src/switches.rs`,
  `every_translator_switch_is_in_the_shader_cache_key`): every `VICTORY_*`
  switch the translator (`crates/victory-shader/src`) reads outside test code
  must be in the list of switches of the shader cache's key
  (`crates/victory-gpu/src/shader_cache.rs`, `fn build`). A switch that
  changes the emitted code but is not in the key serves stale translations.
  Switches the key takes through a call (`loop_limit()`, `group_sync()`,
  `shader_opt()`, `host_settings()`) are in `THROUGH_CALLS`; switches that
  only print or pick the offline tools' device are in `DIAGNOSTICS`, each with
  its reason.
- **English sources** (`xtask/src/english.rs`, `sources_are_in_english`):
  comments, literals and names in `crates/`, `apps/` and `xtask/` hold no
  letters outside ASCII. Symbols, Greek letters and the micro sign are fine.
  A string whose bytes matter writes `\u{...}` escapes, or goes on `ALLOWED`.
- **Documentation links** (`xtask/src/doc_links.rs`,
  `documentation_links_point_at_files_that_exist`): every relative link in a
  Markdown file points at a file or folder of the repository.
- **Crate manifests** (`xtask/src/manifests.rs`,
  `workspace_crates_inherit_lints_and_settings`): every workspace crate has
  `[lints] workspace = true`, takes `version`, `edition`, `rust-version`,
  `license` and `publish` from the workspace, and takes every dependency from
  `[workspace.dependencies]` (`name.workspace = true`).

## Not checked, on purpose

- **Formatting.** The tree is not `rustfmt`-clean and is not reformatted: it
  would diverge from upstream in every file, and any change to a source file
  of `crates/victory-shader` changes the shader cache key, so every user would
  translate and warm the pipelines again. Format the files you add
  (`rustfmt --edition 2024 <file>`), not the ones you touch.
- **The plain build's documentation.** `cargo doc` without
  `--all-features` warns on a link into a module only the diagnostics build
  has (`victory-gpu`'s `timeline`); the diagnostics build documents both.
