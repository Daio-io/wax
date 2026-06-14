# Interactive Init Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a TTY-only `wax init` setup wizard that selects languages, records scan roots, asks about in-repo registry sources, writes the existing init artifacts, and prints next commands without running scan or discovery.

**Architecture:** Keep durable init writes in `engine/crates/wax-cli/src/commands/init.rs`, but split selection gathering from file generation. Non-interactive mode keeps using CLI flags; interactive mode resolves the pack index, prompts through a small adapter, converts answers into the same init execution path, and only uses registry source roots for final guidance.

**Tech Stack:** Rust, clap, dialoguer, serde_json, standard-library `Command` integration tests already used by `wax-cli`.

---

## Reference Specs

- [Interactive init design](../specs/2026-06-13-interactive-init-design.md)
- [Post-alpha UX plan, Task 1](./2026-05-24-post-alpha-ux-plan.md#phase-1--guided-init)
- [Language packs and distribution](../specs/2026-05-16-language-packs-and-distribution.md)

## When to Start

This plan is an extracted implementation plan for Post-alpha UX Task 1 only. The roadmap marks it as `merged` and as the current `in-progress` implementation plan.

The implementation PR should still tick Post-alpha UX Task 1 in `docs/plans/2026-05-24-post-alpha-ux-plan.md` when all task steps are complete. This docs PR only records the plan.

## File Structure

- Modify `engine/crates/wax-cli/Cargo.toml`
  - Add `dialoguer` for TTY prompts.
- Modify `engine/crates/wax-cli/src/commands/init.rs`
  - Add `InitSelections`, `RegistrySetup`, and prompt abstractions.
  - Refactor `run_init` so non-interactive options and interactive selections converge before config/lockfile writes.
  - Add a test-only prompt implementation for unit tests.
  - Preserve existing non-interactive semantics.
- Modify `engine/crates/wax-cli/src/main.rs`
  - Pass process stdin TTY status into init, if the implementation chooses to detect TTY outside `init.rs`.
  - Keep existing CLI flags unchanged.
- Create `engine/crates/wax-cli/tests/init_interactive.rs`
  - Add CLI-level coverage for non-TTY friendly failure.
  - Add integration coverage when practical for stdout/stderr guidance.
- Modify `README.md`
  - Add a short interactive init section after the current non-interactive init examples.
  - Fix the existing registry file list to mention per-language `.wax/<language-id>.registry.json`.
- Modify `docs/plans/2026-05-24-post-alpha-ux-plan.md`
  - Tick Task 1 and its completed steps in the same implementation PR.
- Modify `docs/plans/README.md`
  - Mark this extracted plan complete or update the active-plan gate when implementation finishes.

## Task 1: Selection Model and Config Roots

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/init.rs`

- [ ] **Step 1: Add failing tests for scan roots from selections**

Add these tests near the existing `init_writes_waxrc_lockfile_and_installs_selected_language` tests in `engine/crates/wax-cli/src/commands/init.rs`:

```rust
#[test]
fn init_writes_interactive_scan_roots() {
    let temp = TestDir::new("init-interactive-scan-roots");
    let artifact_path = temp.path.join("compose.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
    let react_artifact_path = temp.path.join("react.tgz");
    let react_digest = write_pack_artifact(&react_artifact_path, "wax-lang-react");
    let registry_path = temp.path.join("registry.json");
    fs::write(
        &registry_path,
        format!(
            r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}},{{"id":"react","version":"0.2.0","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            file_url(&artifact_path),
            digest,
            file_url(&react_artifact_path),
            react_digest
        ),
    )
    .unwrap();
    let repo_root = temp.path.join("repo");
    fs::create_dir_all(&repo_root).unwrap();
    let mut output = Vec::new();

    run_init(
        InitOptions {
            non_interactive: false,
            languages: Vec::new(),
            no_install: true,
            registry_url: Some(file_url(&registry_path)),
            repo_root: repo_root.clone(),
            target_triple: Some("test-target".to_owned()),
            state_path: Some(temp.path.join("home/state.json")),
            scaffold_registries: true,
            interactive: Some(InitSelections {
                languages: vec![
                    LanguageId::try_from("compose").unwrap(),
                    LanguageId::try_from("react").unwrap(),
                ],
                scan_roots: BTreeMap::from([
                    (
                        LanguageId::try_from("compose").unwrap(),
                        vec![PathBuf::from("android/app/src/main/kotlin")],
                    ),
                    (
                        LanguageId::try_from("react").unwrap(),
                        vec![PathBuf::from("apps/web/src"), PathBuf::from("packages/ui/src")],
                    ),
                ]),
                registry_setup: RegistrySetup::External,
            }),
        },
        &mut output,
    )
    .expect("interactive init");

    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo_root.join(".wax/wax.config.json")).unwrap())
            .unwrap();
    let languages = config["languages"].as_array().unwrap();
    assert_eq!(languages[0]["id"], "compose");
    assert_eq!(
        languages[0]["roots"],
        serde_json::json!(["android/app/src/main/kotlin"])
    );
    assert_eq!(languages[1]["id"], "react");
    assert_eq!(
        languages[1]["roots"],
        serde_json::json!(["apps/web/src", "packages/ui/src"])
    );
}

#[test]
fn init_does_not_persist_registry_source_roots() {
    let temp = TestDir::new("init-registry-roots-not-persisted");
    let artifact_path = temp.path.join("compose.tgz");
    let digest = write_pack_artifact(&artifact_path, "wax-lang-compose");
    let registry_path = temp.path.join("registry.json");
    fs::write(
        &registry_path,
        format!(
            r#"[{{"id":"compose","version":"0.4.2","api_version":1,"targets":{{"test-target":{{"url":"{}","sha256":"{}"}}}}}}]"#,
            file_url(&artifact_path),
            digest
        ),
    )
    .unwrap();
    let repo_root = temp.path.join("repo");
    fs::create_dir_all(&repo_root).unwrap();

    run_init(
        InitOptions {
            non_interactive: false,
            languages: Vec::new(),
            no_install: true,
            registry_url: Some(file_url(&registry_path)),
            repo_root: repo_root.clone(),
            target_triple: Some("test-target".to_owned()),
            state_path: Some(temp.path.join("home/state.json")),
            scaffold_registries: true,
            interactive: Some(InitSelections {
                languages: vec![LanguageId::try_from("compose").unwrap()],
                scan_roots: BTreeMap::from([(
                    LanguageId::try_from("compose").unwrap(),
                    vec![PathBuf::from("app/src/main/kotlin")],
                )]),
                registry_setup: RegistrySetup::InRepository {
                    roots: BTreeMap::from([(
                        LanguageId::try_from("compose").unwrap(),
                        vec![PathBuf::from("design-system/src/main/kotlin")],
                    )]),
                },
            }),
        },
        &mut Vec::new(),
    )
    .expect("interactive init");

    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo_root.join(".wax/wax.config.json")).unwrap())
            .unwrap();
    let language = &config["languages"][0];
    assert_eq!(language["roots"], serde_json::json!(["app/src/main/kotlin"]));
    let serialized = serde_json::to_string(language).unwrap();
    assert!(
        !serialized.contains("design-system/src/main/kotlin"),
        "registry source roots must only appear in next-step output"
    );
}
```

Expected: FAIL because `InitOptions` has no `interactive` field and `InitSelections` / `RegistrySetup` do not exist.

- [ ] **Step 2: Add selection types**

In `engine/crates/wax-cli/src/commands/init.rs`, add these types below `PendingRegistryScaffold`:

```rust
/// Answers collected by the interactive init wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitSelections {
    /// Language pack ids selected by the user.
    pub languages: Vec<LanguageId>,
    /// Scan roots to persist in `.wax/wax.config.json`, keyed by language id.
    pub scan_roots: BTreeMap<LanguageId, Vec<PathBuf>>,
    /// Registry setup mode selected by the user.
    pub registry_setup: RegistrySetup,
}

/// Registry setup answer collected during interactive init.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrySetup {
    /// Registry definitions are managed outside this repository.
    External,
    /// Registry source is in this repository. Roots are used only for printed follow-up commands.
    InRepository {
        /// Source roots for `wax registry discover`, keyed by language id.
        roots: BTreeMap<LanguageId, Vec<PathBuf>>,
    },
}
```

Extend `InitOptions`:

```rust
/// Interactive selections. When present, init uses these answers instead of CLI language flags.
pub interactive: Option<InitSelections>,
```

- [ ] **Step 3: Route languages through selections**

Replace the start of `run_init` with:

```rust
let selections = options.interactive.clone();
if !options.non_interactive && selections.is_none() {
    return Err(InitCommandError::RequiresInteractiveTerminal);
}

let languages = match &selections {
    Some(selections) => dedupe_languages(&selections.languages),
    None => dedupe_languages(&options.languages),
};
if languages.is_empty() {
    return Err(InitCommandError::MissingLanguageSelection);
}
```

Add the new error variant while keeping the old missing-language error:

```rust
/// Interactive init requires a terminal unless scriptable flags are used.
#[error(
    "wax init needs an interactive terminal. For CI or scripts, run: wax init --non-interactive --language <language-id>"
)]
RequiresInteractiveTerminal,
```

Remove or stop using `RequiresNonInteractiveFlag` only after all call sites and tests are updated in Task 3.

- [ ] **Step 4: Add root injection into config generation**

Change the call site:

```rust
let waxrc_contents =
    build_waxrc_contents(&languages, options.scaffold_registries, selections.as_ref())?;
```

Change the signature:

```rust
fn build_waxrc_contents(
    selected: &[LanguageId],
    scaffold_registries: bool,
    selections: Option<&InitSelections>,
) -> Result<String, InitCommandError> {
```

Inside the `for entry in &mut filtered` loop, after registry insertion/removal, add:

```rust
if let Some(selections) = selections
    && let Some(id) = object.get("id").and_then(serde_json::Value::as_str)
{
    let language_id = LanguageId::try_from(id).expect("example template language ids are validated");
    if let Some(roots) = selections.scan_roots.get(&language_id) {
        object.insert(
            "roots".to_owned(),
            serde_json::Value::Array(
                roots
                    .iter()
                    .map(|root| serde_json::Value::String(root.to_string_lossy().to_string()))
                    .collect(),
            ),
        );
    }
}
```

- [ ] **Step 5: Run focused failing/passing test**

Run:

```bash
cd engine
cargo test -p wax-cli init_writes_interactive_scan_roots
cargo test -p wax-cli init_does_not_persist_registry_source_roots
```

Expected: PASS after implementation.

- [ ] **Step 6: Commit Task 1**

```bash
git add engine/crates/wax-cli/src/commands/init.rs
git commit -m "feat: model interactive init selections"
```

## Task 2: Interactive Prompt Adapter

**Files:**
- Modify: `engine/crates/wax-cli/Cargo.toml`
- Modify: `engine/crates/wax-cli/src/commands/init.rs`

- [ ] **Step 1: Add prompt dependency**

In `engine/crates/wax-cli/Cargo.toml`, add:

```toml
dialoguer = "0.11"
```

Run:

```bash
cd engine
cargo check -p wax-cli
```

Expected: PASS and `engine/Cargo.lock` updates with `dialoguer` dependencies.

- [ ] **Step 2: Add failing unit tests for prompt-independent guidance**

Add these tests in `engine/crates/wax-cli/src/commands/init.rs`:

```rust
#[test]
fn registry_discover_guidance_uses_interactive_roots() {
    let selections = InitSelections {
        languages: vec![LanguageId::try_from("compose").unwrap()],
        scan_roots: BTreeMap::from([(
            LanguageId::try_from("compose").unwrap(),
            vec![PathBuf::from("app/src/main/kotlin")],
        )]),
        registry_setup: RegistrySetup::InRepository {
            roots: BTreeMap::from([(
                LanguageId::try_from("compose").unwrap(),
                vec![PathBuf::from("design-system/src/main/kotlin")],
            )]),
        },
    };

    let mut output = Vec::new();
    write_next_steps(Some(&selections), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("wax registry discover --language compose --root design-system/src/main/kotlin"));
    assert!(output.contains("wax scan"));
}

#[test]
fn external_registry_guidance_explains_registry_setup() {
    let selections = InitSelections {
        languages: vec![LanguageId::try_from("react").unwrap()],
        scan_roots: BTreeMap::from([(
            LanguageId::try_from("react").unwrap(),
            vec![PathBuf::from("apps/web/src")],
        )]),
        registry_setup: RegistrySetup::External,
    };

    let mut output = Vec::new();
    write_next_steps(Some(&selections), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(".wax/react.registry.json"));
    assert!(output.contains("wax scan"));
    assert!(!output.contains("wax registry discover"));
}

#[test]
fn empty_registry_source_roots_print_guidance_without_root_args() {
    let selections = InitSelections {
        languages: vec![LanguageId::try_from("compose").unwrap()],
        scan_roots: BTreeMap::from([(
            LanguageId::try_from("compose").unwrap(),
            vec![PathBuf::from("app/src/main/kotlin")],
        )]),
        registry_setup: RegistrySetup::InRepository {
            roots: BTreeMap::from([(LanguageId::try_from("compose").unwrap(), Vec::new())]),
        },
    };

    let mut output = Vec::new();
    write_next_steps(Some(&selections), &mut output).unwrap();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("No registry source roots were provided for compose"));
    assert!(!output.contains("--root"));
    assert!(output.contains("wax scan"));
}
```

Expected: FAIL because `write_next_steps` does not exist.

- [ ] **Step 3: Implement final guidance helper**

Add this helper near `run_init`:

```rust
fn write_next_steps(
    selections: Option<&InitSelections>,
    writer: &mut impl Write,
) -> Result<(), InitCommandError> {
    let Some(selections) = selections else {
        return Ok(());
    };

    (|| -> io::Result<()> {
        match &selections.registry_setup {
        RegistrySetup::External => {
            writeln!(writer)?;
            writeln!(writer, "Registry setup:")?;
            for language_id in &selections.languages {
                writeln!(
                    writer,
                    "- Populate .wax/{}.registry.json or update that language's registry source before scanning.",
                    language_id.as_str()
                )?;
            }
            writeln!(writer, "Then run `wax scan`.")?;
        }
        RegistrySetup::InRepository { roots } => {
            writeln!(writer)?;
            writeln!(writer, "Next, populate registries from your design-system source:")?;
            for language_id in &selections.languages {
                if let Some(language_roots) = roots.get(language_id) {
                    if language_roots.is_empty() {
                        writeln!(
                            writer,
                            "No registry source roots were provided for {}; populate .wax/{}.registry.json or rerun wax registry discover with --root.",
                            language_id.as_str(),
                            language_id.as_str()
                        )?;
                    } else {
                        for root in language_roots {
                            writeln!(
                                writer,
                                "wax registry discover --language {} --root {}",
                                language_id.as_str(),
                                root.display()
                            )?;
                        }
                    }
                }
            }
            writeln!(writer, "Then run `wax scan`.")?;
        }
        }

        Ok(())
    })()
    .map_err(|source| InitCommandError::Io {
        context: "write init guidance".to_owned(),
        source,
    })
}
```

Call it after the existing `initialized wax in ...` line:

```rust
write_next_steps(selections.as_ref(), writer)?;
```

- [ ] **Step 4: Add prompt abstraction and dialoguer implementation**

Add this trait and dialoguer-backed implementation:

```rust
trait InitPrompts {
    fn select_languages(
        &mut self,
        manifests: &[RegistryManifest],
    ) -> Result<Vec<LanguageId>, InitCommandError>;

    fn scan_roots(&mut self, language_id: &LanguageId) -> Result<Vec<PathBuf>, InitCommandError>;

    fn registry_in_repo(&mut self) -> Result<bool, InitCommandError>;

    fn registry_roots(&mut self, language_id: &LanguageId) -> Result<Vec<PathBuf>, InitCommandError>;
}

struct DialoguerInitPrompts;
```

Implement comma-splitting for roots:

```rust
fn parse_roots(input: &str) -> Vec<PathBuf> {
    input
        .split(',')
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .collect()
}
```

Use `dialoguer::MultiSelect`, `dialoguer::Input`, and `dialoguer::Confirm` in `DialoguerInitPrompts`.

Sort manifest choices so `compose` appears first when present, followed by the remaining ids alphabetically:

```rust
fn sorted_language_manifests(manifests: &[RegistryManifest]) -> Vec<&RegistryManifest> {
    let mut sorted = manifests.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| match (left.id.as_str(), right.id.as_str()) {
        ("compose", "compose") => std::cmp::Ordering::Equal,
        ("compose", _) => std::cmp::Ordering::Less,
        (_, "compose") => std::cmp::Ordering::Greater,
        _ => left.id.as_str().cmp(right.id.as_str()),
    });
    sorted
}
```

- [ ] **Step 5: Add selection collection function**

Add:

```rust
fn collect_interactive_selections(
    manifests: &[RegistryManifest],
    prompts: &mut impl InitPrompts,
) -> Result<InitSelections, InitCommandError> {
    let languages = dedupe_languages(&prompts.select_languages(manifests)?);
    if languages.is_empty() {
        return Err(InitCommandError::MissingLanguageSelection);
    }

    let mut scan_roots = BTreeMap::new();
    for language_id in &languages {
        scan_roots.insert(language_id.clone(), prompts.scan_roots(language_id)?);
    }

    let registry_setup = if prompts.registry_in_repo()? {
        let mut roots = BTreeMap::new();
        for language_id in &languages {
            roots.insert(language_id.clone(), prompts.registry_roots(language_id)?);
        }
        RegistrySetup::InRepository { roots }
    } else {
        RegistrySetup::External
    };

    Ok(InitSelections {
        languages,
        scan_roots,
        registry_setup,
    })
}
```

- [ ] **Step 6: Add mocked prompt tests**

Add a test prompt implementation under `#[cfg(test)]`:

```rust
struct MockInitPrompts {
    languages: Vec<LanguageId>,
    scan_roots: BTreeMap<LanguageId, Vec<PathBuf>>,
    registry_in_repo: bool,
    registry_roots: BTreeMap<LanguageId, Vec<PathBuf>>,
}

impl InitPrompts for MockInitPrompts {
    fn select_languages(
        &mut self,
        _manifests: &[RegistryManifest],
    ) -> Result<Vec<LanguageId>, InitCommandError> {
        Ok(self.languages.clone())
    }

    fn scan_roots(&mut self, language_id: &LanguageId) -> Result<Vec<PathBuf>, InitCommandError> {
        Ok(self.scan_roots.get(language_id).cloned().unwrap_or_default())
    }

    fn registry_in_repo(&mut self) -> Result<bool, InitCommandError> {
        Ok(self.registry_in_repo)
    }

    fn registry_roots(&mut self, language_id: &LanguageId) -> Result<Vec<PathBuf>, InitCommandError> {
        Ok(self.registry_roots.get(language_id).cloned().unwrap_or_default())
    }
}
```

Add:

```rust
#[test]
fn collect_interactive_selections_uses_mocked_prompt_answers() {
    let manifests = vec![
        RegistryManifest {
            id: LanguageId::try_from("compose").unwrap(),
            version: "0.4.2".to_owned(),
            api_version: 1,
            targets: BTreeMap::new(),
        },
        RegistryManifest {
            id: LanguageId::try_from("react").unwrap(),
            version: "0.2.0".to_owned(),
            api_version: 1,
            targets: BTreeMap::new(),
        },
    ];
    let mut prompts = MockInitPrompts {
        languages: vec![LanguageId::try_from("compose").unwrap()],
        scan_roots: BTreeMap::from([(
            LanguageId::try_from("compose").unwrap(),
            vec![PathBuf::from("app/src/main/kotlin")],
        )]),
        registry_in_repo: true,
        registry_roots: BTreeMap::from([(
            LanguageId::try_from("compose").unwrap(),
            vec![PathBuf::from("design-system/src/main/kotlin")],
        )]),
    };

    let selections = collect_interactive_selections(&manifests, &mut prompts).unwrap();

    assert_eq!(selections.languages, vec![LanguageId::try_from("compose").unwrap()]);
    assert_eq!(
        selections.scan_roots[&LanguageId::try_from("compose").unwrap()],
        vec![PathBuf::from("app/src/main/kotlin")]
    );
    assert!(matches!(selections.registry_setup, RegistrySetup::InRepository { .. }));
}
```

- [ ] **Step 7: Run focused tests**

Run:

```bash
cd engine
cargo test -p wax-cli registry_discover_guidance_uses_interactive_roots
cargo test -p wax-cli external_registry_guidance_explains_registry_setup
cargo test -p wax-cli empty_registry_source_roots_print_guidance_without_root_args
cargo test -p wax-cli collect_interactive_selections_uses_mocked_prompt_answers
cargo test -p wax-cli parse_roots
```

Expected: PASS.

- [ ] **Step 8: Commit Task 2**

```bash
git add engine/Cargo.lock engine/crates/wax-cli/Cargo.toml engine/crates/wax-cli/src/commands/init.rs
git commit -m "feat: add interactive init prompts"
```

## Task 3: CLI TTY Wiring and Non-TTY Failure

**Files:**
- Modify: `engine/crates/wax-cli/src/commands/init.rs`
- Modify: `engine/crates/wax-cli/src/main.rs`
- Create: `engine/crates/wax-cli/tests/init_interactive.rs`

- [ ] **Step 1: Add failing non-TTY integration test**

Create `engine/crates/wax-cli/tests/init_interactive.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wax-cli-{name}-{nonce}"));
        fs::create_dir_all(&path).expect("create temp dir");
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn init_without_non_interactive_requires_tty() {
    let root = TestDir::new("init-non-tty");
    let repo = root.path.join("repo");
    fs::create_dir_all(&repo).expect("create repo fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_wax"))
        .arg("init")
        .arg("--no-install")
        .arg("--repo-root")
        .arg(&repo)
        .output()
        .expect("run wax init");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("wax init needs an interactive terminal"),
        "stderr was: {stderr}"
    );
    assert!(
        stderr.contains("wax init --non-interactive --language <language-id>"),
        "stderr was: {stderr}"
    );
}
```

Run:

```bash
cd engine
cargo test -p wax-cli --test init_interactive
```

Expected: FAIL until TTY detection and error message are wired.

- [ ] **Step 2: Add TTY-aware entry point**

In `engine/crates/wax-cli/src/commands/init.rs`, import:

```rust
use std::io::IsTerminal;
```

Add:

```rust
/// Runs `wax init` using the process terminal for interactive prompts.
pub fn run_init_cli(options: InitOptions, writer: &mut impl Write) -> Result<(), InitCommandError> {
    if options.non_interactive {
        return run_init(options, writer);
    }

    let config_path = options.repo_root.join(PREFERRED_CONFIG_RELATIVE_PATH);
    if config_path.exists() {
        return Err(InitCommandError::WaxConfigAlreadyExists { path: config_path });
    }

    if !io::stdin().is_terminal() {
        return Err(InitCommandError::RequiresInteractiveTerminal);
    }

    let registry_url = resolve_registry_url(options.registry_url.clone())?;
    let manifests = fetch_pack_index(&registry_url)?;
    let mut prompts = DialoguerInitPrompts;
    let selections = collect_interactive_selections(&manifests, &mut prompts)?;
    run_init(
        InitOptions {
            interactive: Some(selections),
            registry_url: Some(registry_url),
            ..options
        },
        writer,
    )
}
```

Keep `run_init` testable with injected selections and no direct terminal dependency.

- [ ] **Step 3: Switch `main.rs` to CLI entry point**

Change the import:

```rust
use commands::init::{InitOptions, run_init_cli};
```

Change command dispatch:

```rust
Commands::Init(args) => run_init_cli(
    InitOptions {
        non_interactive: args.non_interactive,
        languages: args.languages,
        no_install: args.no_install,
        registry_url: args.registry,
        repo_root: args.repo_root,
        target_triple: args.target,
        state_path: None,
        scaffold_registries: !args.no_scaffold_registries,
        interactive: None,
    },
    &mut stdout,
)
.map_err(Into::into),
```

- [ ] **Step 4: Update existing tests for new field**

For every existing `InitOptions { ... }` literal in `engine/crates/wax-cli/src/commands/init.rs`, add:

```rust
interactive: None,
```

Update the old `requires_non_interactive_flag_for_scriptable_init` test to assert `RequiresInteractiveTerminal` instead of `RequiresNonInteractiveFlag`, or rename it to:

```rust
#[test]
fn init_without_interactive_answers_requires_terminal() {
    // existing test body, with interactive: None
    assert!(matches!(err, InitCommandError::RequiresInteractiveTerminal));
}
```

- [ ] **Step 5: Run init tests**

Run:

```bash
cd engine
cargo test -p wax-cli init_
cargo test -p wax-cli --test init_interactive
```

Expected: PASS.

- [ ] **Step 6: Commit Task 3**

```bash
git add engine/crates/wax-cli/src/commands/init.rs engine/crates/wax-cli/src/main.rs engine/crates/wax-cli/tests/init_interactive.rs
git commit -m "feat: wire interactive init cli"
```

## Task 4: Documentation, Plan Checkbox, and Verification

**Files:**
- Modify: `README.md`
- Modify: `docs/plans/2026-05-24-post-alpha-ux-plan.md`
- Modify: `docs/plans/README.md`
- Modify: `docs/specs/2026-05-16-language-packs-and-distribution.md`

- [x] **Step 1: Update README init docs**

In `README.md`, after the non-interactive examples, add:

```markdown
For local setup, `wax init` can guide you through the same choices:

```bash
wax init
```

The wizard asks which language packs to enable, which source roots Wax should scan for each language, and whether your design-system registry source lives in this repository. If the registry source is in the repo, init prints the `wax registry discover` commands to run next. It does not run discovery or scan automatically.
```

Update the file list under `` `wax init` writes:`` to:

```markdown
- `.wax/wax.config.json`
- `.wax/wax.lock.json`
- `.wax/<language-id>.registry.json`
```

- [x] **Step 2: Tick post-alpha UX Task 1**

In `docs/plans/2026-05-24-post-alpha-ux-plan.md`, change Task 1 and all four steps from unchecked to checked:

```markdown
### - [x] Task 1: Interactive `wax init` TTY wizard
...
- [x] **Step 1: Choose prompt library and document non-interactive invariant**
- [x] **Step 2: Prompt for language (Compose-first), scan roots, and registry source roots**
- [x] **Step 3: Fall back to current behavior when not a TTY**
- [x] **Step 4: Manual smoke + unit tests with mocked stdin**
```

Replace the old optional-scan wording with a short note below Step 2:

```markdown
Implementation keeps init setup-only: it asks for scan roots and registry source roots, then prints registry-discovery and scan next steps instead of running either command automatically.
```

- [x] **Step 3: Update roadmap and older spec references**

In `docs/plans/README.md`, update order 9 after the implementation PR completes:

```markdown
| 9 | Interactive init wizard | [2026-06-13-interactive-init.md](./2026-06-13-interactive-init.md) | `merged` | `complete` | — |
```

In `docs/specs/2026-05-16-language-packs-and-distribution.md`, replace the sentence that says interactive init remains deferred with:

```markdown
Interactive init is implemented by the Post-alpha UX Task 1 extraction. It guides TTY users through language selection, scan roots, and registry next steps while preserving `--non-interactive` for scripts.
```

- [x] **Step 4: Run focused verification**

Run:

```bash
cd engine
cargo fmt --all --check
cargo test -p wax-cli init_
cargo test -p wax-cli --test init_command
cargo test -p wax-cli --test init_interactive
```

Expected: PASS.

- [x] **Step 5: Run broad verification if shared behavior changed**

If the implementation touched `wax-core`, config parsing, lockfile serialization, or language install behavior, also run:

```bash
cd engine
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: PASS. If this is not run because the implementation stayed inside `wax-cli`, note that in the PR.

- [x] **Step 6: Commit Task 4**

```bash
git add README.md docs/plans/2026-05-24-post-alpha-ux-plan.md docs/plans/README.md docs/specs/2026-05-16-language-packs-and-distribution.md
git commit -m "docs: document interactive init"
```

## Execution Notes

- Do not implement automatic `wax registry discover` in this plan.
- Do not implement automatic `wax scan` in this plan.
- Do not persist registry source roots in `.wax/wax.config.json`.
- Keep prompt dependency local to `wax-cli`.
- Preserve `wax init --non-interactive --language <id>` behavior for CI and scripts.

## Final PR Verification

Before opening the implementation PR, run:

```bash
cd engine
cargo fmt --all --check
cargo test -p wax-cli init_
cargo test -p wax-cli --test init_command
cargo test -p wax-cli --test init_interactive
```

Expected: PASS.
