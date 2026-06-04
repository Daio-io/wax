# Registry Discovery and Skill-Assisted Sync Design

## Summary

Wax should help design-system maintainers create `.wax/wax.registry.json` from a design-system package without making AI a runtime dependency. The first version adds a deterministic `wax registry discover` command that scans a design-system root, writes the default registry file, and refuses to overwrite an existing registry unless `--force` is supplied. A companion Agent Skill, `wax-registry-sync`, can wrap that deterministic command with source inspection, human confirmation, and validation.

This feature targets design-system repositories or packages. It does not infer registries from consuming app usage.

## Goals

- Let maintainers bootstrap `.wax/wax.registry.json` from a design-system package.
- Keep the default CLI path deterministic, scriptable, and useful without AI.
- Write to the canonical `.wax/wax.registry.json` path by default.
- Prevent accidental registry replacement by refusing to overwrite without `--force`.
- Provide `--dry-run` so users can preview generated registry JSON on stdout.
- Make false-positive risk explicit instead of hiding it behind “magic” generation.
- Define a proper Agent Skill workflow that can review, refine, and update the registry.

## Non-Goals

- Wax will not infer a design-system registry from consuming app repositories in this phase.
- Wax will not make AI part of `wax scan`, `wax validate`, or language-pack execution.
- Wax will not attempt perfect semantic export analysis in v1.
- Wax will not introduce a hosted registry-authoring service.
- Wax will not change the registry schema beyond fields already accepted by language packs unless implementation discovers a required compatibility gap.

## User Experience

The primary command writes the default registry:

```bash
wax registry discover --language compose
```

If `.wax/wax.registry.json` does not exist, Wax writes it and prints a concise summary:

```text
Discovered 14 compose registry components from 1 root.
Wrote .wax/wax.registry.json.
Review before committing: deterministic discovery may include false positives.
Run `wax validate` to verify repository configuration.
```

If `.wax/wax.registry.json` already exists, Wax stops:

```text
.wax/wax.registry.json already exists.
Re-run with --force to replace it, or use --dry-run to preview discovered components.
```

Users can preview output without writing files:

```bash
wax registry discover --language compose --dry-run
```

`--dry-run` prints the registry JSON to stdout and writes diagnostics to stderr so the JSON remains scriptable.

Users can point discovery at a specific package or source root:

```bash
wax registry discover --language compose --root design-system/src/main/kotlin
```

Users can intentionally replace an existing registry:

```bash
wax registry discover --language compose --force
```

`--force` replaces `.wax/wax.registry.json` only after discovery succeeds. A failed discovery must leave the existing registry unchanged.

## Root Selection

Root selection should be simple and predictable. `--root` is the primary path for this command because registry discovery targets a design-system package, not a consuming app:

- If `--root` is supplied, Wax scans that repo-relative path.
- If `--root` is omitted, Wax reads the enabled language entry from `.wax/wax.config.json` or legacy `.waxrc` and uses that language's configured roots as a convenience fallback.
- If no config exists or the selected language has no usable roots, Wax fails with an example command using `--root`.
- If multiple configured roots exist, Wax scans all of them.
- Non-interactive runs never prompt.

Config roots are usually scan targets and may point at app code rather than a design-system source package. When discovery falls back to config roots, Wax should warn users to prefer `--root path/to/design-system` if the configured roots are not the design-system package. This keeps v1 scriptable. A later interactive registry wizard can help choose roots, but it is not needed for the first implementation.

## Registry Output Contract

Discovery writes schema version 1 registry JSON:

```json
{
  "schema_version": 1,
  "components": [
    {
      "id": "ds.primary-button",
      "symbol": "PrimaryButton",
      "aliases": []
    }
  ]
}
```

For v1, generated component ids use a deterministic slug:

```text
ds.<kebab-case-symbol>
```

Generated output must be stable across runs for the same source tree:

- Components are sorted by `symbol`.
- Duplicate symbols collapse to one component.
- JSON formatting is deterministic.
- Empty optional arrays may be omitted if existing registry fixtures already prefer omission.

The command may include only fields supported by current registry validators and language packs. Rich metadata such as category, props, slots, events, source locations, and confidence can be added in later schema work or emitted only in diagnostics until the registry contract supports it consistently.

## Compose Discovery Rules

The first language implementation is Compose. The Compose discovery pass should identify likely public design-system components using conservative syntax rules:

- Include top-level Kotlin functions annotated with `@Composable`.
- Include functions that are public by default or explicitly `public`.
- Exclude functions marked `private` or `internal`.
- Exclude functions whose names do not look like component symbols, such as lowercase helper functions.
- Exclude duplicate symbols after sorting.
- Support common source extensions under configured roots, primarily `.kt`.

This is intentionally not a full Kotlin semantic analyzer. It is a deterministic bootstrap tool that can produce false positives. The command output and docs must say that plainly.

Future improvements can add language-specific candidate diagnostics, source locations, package filters, exported API analysis, and richer registry metadata.

`wax registry draft` remains deferred. This phase ships one deterministic authoring command, `wax registry discover`, plus the AI-assisted skill workflow around it.

## AI-Assisted Skill

Wax should publish Agent Skills under `plugins/wax/skills/<skill-name>/`. The first skill is `wax-registry-sync`. Skills in the open skills ecosystem are reusable agent capabilities defined by `SKILL.md` files with YAML frontmatter containing `name` and `description`. Install individual skills via [skills.sh](https://skills.sh) (`npx skills add Daio-io/wax --skill wax-registry-sync`) into project `.agents/skills/` or global agent paths, or install the grouped Claude plugin (`/plugin marketplace add Daio-io/wax`, `/plugin install wax@wax-skills`).

The skill is not part of the Wax runtime. It is an authoring assistant around source-controlled registry files.

The skill workflow:

1. Inspect Wax config and determine the target language and roots.
2. Check whether `.wax/wax.registry.json` already exists.
3. Run `wax registry discover --language <id> --dry-run`.
4. Compare dry-run output with the existing registry when present.
5. Inspect design-system source files for ambiguous inclusions or exclusions.
6. Ask focused questions only for ambiguous cases.
7. Write or update `.wax/wax.registry.json`.
8. Use `--force` only after showing the intended replacement or diff.
9. Run `wax validate`.
10. Run `wax language update` when registry locks need refreshing.

The skill should treat an existing registry as user-owned source of truth. It should refine or replace it deliberately, never blindly.

## Architecture

`wax-cli` owns the user-facing `registry discover` command. `wax-core` should own reusable registry discovery orchestration and file-writing safety so tests do not depend on CLI output parsing.

Language-specific candidate extraction should stay near the language implementation. For Compose, the most direct first step is to add a discovery module in `wax-lang-compose` or a shared language API entry point if the implementation plan needs cross-pack extensibility. The design intent is that future `wax-lang-react` or other packs can implement their own deterministic discovery rules without changing the CLI contract.

The command data flow:

1. Parse CLI flags.
2. Resolve repo files and language config.
3. Determine discovery roots.
4. Run the language-specific discovery pass.
5. Convert discovered symbols into registry components.
6. Validate the generated registry JSON shape.
7. If `--dry-run`, print JSON to stdout and diagnostics to stderr.
8. Otherwise, write `.wax/wax.registry.json` atomically.
9. Refuse to overwrite unless `--force` is set.

## Error Handling

Discovery fails for:

- unsupported language id
- missing repo root or config when `--root` is omitted
- missing configured roots
- `--root` paths that do not exist
- roots that escape the repository when repo-relative paths are required
- no discoverable components
- existing `.wax/wax.registry.json` without `--force`
- malformed generated registry JSON

Warnings should be emitted for:

- likely false positives due to deterministic discovery
- skipped private or internal symbols when counts are available
- multiple roots scanned
- existing registry differences in the skill-assisted path

## Testing

CLI tests should cover:

- `--dry-run` prints valid registry JSON to stdout and writes no file.
- default discovery writes `.wax/wax.registry.json`.
- existing registry refuses overwrite.
- `--force` replaces an existing registry.
- `--root` uses the supplied source path.
- omitted `--root` uses config roots.
- missing roots fail with a command example.

Compose discovery tests should cover:

- public top-level composables are included.
- private and internal composables are excluded.
- lowercase helper composables are excluded.
- duplicate symbols are stable and de-duplicated.
- output ordering is deterministic.

Skill tests can be documentation and fixture based:

- the skill runs dry-run before writing.
- the skill refuses blind overwrite.
- the skill validates after writing.
- the skill refreshes registry locks when needed.

## Documentation

Docs should update:

- `README.md` with a registry discovery quick-start.
- `docs/plans/README.md` to mark this as the order 4 registry authoring phase before post-alpha UX.
- The implementation plan with one task per focused PR.

User-facing docs must keep the false-positive warning visible. This is a feature, not a footnote: deterministic discovery is a bootstrap aid, and the committed registry remains the source of truth.
