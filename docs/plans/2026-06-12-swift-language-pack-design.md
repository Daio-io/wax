# SwiftUI Language Pack Design

## Summary

Wax should add `wax-lang-swift` as a production parser-backed language pack for SwiftUI projects. The first version targets Compose-level capability: load a configured design-system registry, scan configured Swift source roots, discover local SwiftUI components, resolve registry-backed usage sites by static call names, support registry discovery through the generic discover protocol, and emit deterministic `ScanFacts`.

SwiftUI support should use `tree-sitter-swift`. The active maintained grammar is the `tree-sitter-swift` crate from `alex-pinkus/tree-sitter-swift`; the older `tree-sitter/tree-sitter-swift` repository is abandoned in favor of that grammar.

## Goals

- Add `wax-lang-swift` as an installable language pack and stdio binary.
- Match Compose's scan behavior for SwiftUI: registry components, local components, usage sites, metrics, counts, diagnostics, and partial status.
- Support both common SwiftUI design-system shapes:
  - `struct Name: View` components.
  - `func Name(...) -> some View` factory components.
- Support direct and simple member-qualified SwiftUI usage calls.
- Support per-language registry discovery through the existing `discover` wire protocol.
- Keep the engine pack-agnostic; Swift emits facts only.
- Keep scan output deterministic and CI-friendly.

## Non-Goals

- Swift v1 will not run SwiftPM, Xcode, SourceKit, or the Swift compiler.
- Swift v1 will not perform full module or import resolution.
- Swift v1 will not type-check aliases, generics, conditional compilation, or overloads.
- Swift v1 will not infer design-system registry entries from app usage.
- Swift v1 will not introduce a Swift-specific report schema.
- Swift v1 will not model every result-builder or custom DSL pattern.
- Swift v1 will not add Swift-specific config beyond the existing registry and roots fields.

## Product Decision

Swift v1 should use a Compose-parity tree-sitter scanner with one Swift-specific improvement: in addition to unqualified calls such as `PrimaryButton(...)`, it should count simple member-qualified calls such as `DesignSystem.PrimaryButton(...)` by matching the final member name against the registry symbol and aliases.

This keeps the first implementation contained and predictable while handling a common SwiftUI design-system call style. A React-style resolver that traces imports, SwiftPM packages, and Xcode modules is intentionally deferred.

## Configuration

Swift uses the existing language config shape:

```json
{
  "id": "swift",
  "enabled": true,
  "registry": ".wax/swift.registry.json",
  "roots": ["App/Sources", "DesignSystem/Sources"]
}
```

Rules:

- `registry` is the repo-relative design-system registry path.
- Legacy `design_system_registry` remains accepted for compatibility.
- `roots` is a non-empty array of repo-relative source roots or Wax root patterns.
- Paths must be repo-relative and must not contain parent-directory segments.
- Empty config returns scaffold facts for contributor smoke compatibility.
- No Swift-specific `ignore`, package, alias, or module config is included in v1.

## Registry Matching

Swift consumes the same simple registry fields used by the existing language packs, including the multi-language `targets` convention already used by React:

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton",
      "aliases": ["PrimaryCTA"],
      "targets": ["swift"]
    }
  ]
}
```

Matching rules:

- `symbol` is the canonical registry symbol.
- `aliases` are alternate source names that resolve to the canonical registry symbol.
- `targets` limits availability to language ids when present. Missing or null `targets` means the component is available to every language pack.
- Swift excludes registry components whose `targets` array is present and does not contain `swift`.
- A usage resolved through an alias emits the observed source symbol as `UsageSite.symbol` and the canonical symbol as `UsageSite.registry_symbol`.

If Compose has not gained `targets` filtering by the time Swift implementation starts, add or confirm target filtering consistently across the language packs in the implementation plan.

## Architecture

`wax-lang-swift` mirrors the Compose pack's public shape:

```text
ScanRequest
  -> parse Swift config
  -> load registry symbols and aliases
  -> resolve roots
  -> collect .swift files
  -> parse with tree-sitter-swift
  -> detect local SwiftUI components
  -> detect registry-backed usage calls
  -> emit ScanFacts

DiscoverRequest
  -> resolve discover roots
  -> collect .swift files
  -> parse with tree-sitter-swift
  -> detect public/internal SwiftUI design-system symbols
  -> emit discover_symbols response
```

Suggested crate structure:

- `engine/crates/wax-lang-swift/src/lib.rs` exposes `SwiftLanguage`, `SwiftScanError`, `SwiftDiscoverError`, and public scan/discover helpers.
- `engine/crates/wax-lang-swift/src/bin/wax-lang-swift.rs` routes `scan` and `discover` wire requests.
- `engine/crates/wax-lang-swift/src/swift_ast.rs` owns parser setup, Swift file collection, parse helpers, and tree-sitter AST utilities.
- `engine/crates/wax-lang-swift/src/tree_sitter_scan.rs` owns scan config parsing, registry loading, root resolution, extraction, diagnostics, and fact assembly.
- `engine/crates/wax-lang-swift/src/discover.rs` owns registry symbol discovery.
- `engine/crates/wax-lang-swift/src/component_detect.rs` is optional if SwiftUI predicates become large enough to share between scan and discover.

The engine and CLI should need no Swift-specific scan logic. Release, install, and pack-index integration should be handled as release-surface changes after the pack behavior is tested.

## Local Component Discovery

Swift v1 discovers local SwiftUI components in app roots when either shape is present:

```swift
struct ProfileCard: View {
    var body: some View {
        Text("Profile")
    }
}

func PrimaryButton(title: String) -> some View {
    Button(title) {}
}
```

Detection rules:

- Detect `struct` declarations whose name starts with an uppercase ASCII letter, whose inheritance/conformance clause contains `View`, and whose members include a `body` property returning or declaring `some View`.
- Detect `func` declarations whose name starts with an uppercase ASCII letter and whose return type is `some View`.
- Emit each local component with a stable id, source symbol, and one-based source location.

Skip:

- lowercase declarations;
- declarations that do not conform to `View` and do not return `some View`;
- `private` and `fileprivate` declarations for registry discovery;
- arbitrary properties returning `some View` outside a `View` struct, except `body` as evidence for struct detection;
- dynamic factories, typealiases, inferred return types, and compiler-resolved overloads.

For scan-local components, include repo-local SwiftUI components regardless of visibility, matching Compose's app-source behavior. For registry discovery, only emit public or package/internal design-system symbols and skip `private` / `fileprivate`.

## Usage Extraction

Swift v1 extracts registry-backed call or construction sites:

```swift
PrimaryButton(title: "Save")
Card {
    Text("Details")
}
DesignSystem.PrimaryButton(title: "Save")
DS.Card { Text("Details") }
```

Resolution rules:

1. For an unqualified call, match the callee name against registry symbols and aliases.
2. For a simple member-qualified call, match the final member name against registry symbols and aliases.
3. Emit a `UsageSite` only when the observed name resolves to a registry symbol.
4. Emit `MatchStatus::Resolved`.
5. Sort usage sites deterministically by file, line, and symbol.

Skip:

- comments and string literals;
- identifier mentions without call syntax;
- lowercase SwiftUI built-ins unless the registry explicitly contains them;
- qualified chains that tree-sitter cannot reduce to a static final member;
- dynamic construction, typealias resolution, and compiler-only inference.

## Registry Discovery

`wax registry discover --language swift` should use the existing generic discover protocol and the installed Swift pack. Discovery receives repo-relative roots from the engine and returns sorted symbol names plus diagnostics.

Discovery emits:

- public `struct Name: View` declarations with a `body: some View`;
- package/internal `struct Name: View` declarations with a `body: some View`;
- public `func Name(...) -> some View` declarations;
- package/internal `func Name(...) -> some View` declarations.

Discovery skips:

- `private` declarations;
- `fileprivate` declarations;
- lowercase declarations;
- parse-failed files, which should fail discovery consistently with Compose/React discovery behavior.

## Error Semantics

Fatal scan errors return a wire error and no `ScanFacts`:

- invalid language id;
- malformed config value types;
- missing required `registry` or `roots` when Swift scan config is present;
- absolute paths or parent-directory escapes;
- unreadable or malformed registry JSON;
- tree-sitter parser initialization failure.

Recoverable scan gaps return `ScanFacts` with `Partial` status and diagnostics:

- configured literal roots missing;
- configured wildcard roots matching nothing;
- Swift source parse failures.

Stable diagnostic codes:

- `swift_scaffold`
- `root_not_found`
- `root_glob_not_found`
- `parse_failed`

`Complete` means all resolved roots and collected files were processed without known gaps.

## Testing

Unit tests should cover:

- config validation and scaffold mode;
- registry loading, aliases, and `targets` filtering;
- parser initialization and permissive parse failure handling;
- local `struct Name: View` detection;
- local `func Name(...) -> some View` detection;
- direct usage calls;
- trailing-closure usage calls;
- member-qualified usage calls;
- alias usage resolution;
- comments and string literals excluded;
- lowercase and non-View declarations skipped;
- discovery skips `private` and `fileprivate` symbols.

Integration tests should cover:

- golden scan fixture under `engine/crates/wax-lang-swift/tests/fixtures/small`;
- registry discovery fixture under `engine/crates/wax-lang-swift/tests/fixtures/discover`;
- stdio scan success;
- stdio discover success;
- stdio typed errors for unsupported API versions and invalid requests;
- config validation through the pack binary.

Focused implementation checks:

```bash
cd engine
cargo test -p wax-lang-swift
cargo clippy -p wax-lang-swift --all-targets -- -D warnings
```

Broad release/integration checks:

```bash
cd engine
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/test-generate-pack-index.sh
```

## Release And Documentation Surfaces

Swift promotion should update the same release and install surfaces as React:

- `engine/Cargo.toml` workspace members and release metadata.
- `.github/workflows/release.yml` and release workflow checks.
- `scripts/build-release.sh`.
- `scripts/generate-pack-index.sh` and `scripts/test-generate-pack-index.sh`.
- pack index fixtures under `engine/fixtures/registry/`.
- README language-pack docs and examples.
- optional npm wrapper metadata if it enumerates bundled language-pack names.

These changes should be separate implementation tasks after the scanner and discover behavior are covered by tests.

## Deferred Work

- SwiftPM/Xcode module and import resolution.
- SourceKit-backed type checking.
- Swift-specific ignore patterns.
- Conditional compilation evaluation.
- Richer result-builder extraction.
- Cross-language registry merging.
- Multi-hop qualified or typealias resolution.
