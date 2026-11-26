# Public registry distribution options

**Status:** Bundle consumption and explicit revision adoption implemented; remaining pilot coverage is tracked in Linear. See the
[public registry catalogue ADR](../adr/2026-11-16-public-registry-catalogue.md).

A public design system needs a way to publish its registries so other
repositories can pin and consume them. This spec records the options
considered and the one selected for Phase 1.

## Options considered

### Rejected: public bundles in the main Wax repository

The previous proposal kept public design-system artifacts in the engine's
main repository. This couples independently versioned system bundles to
engine development and release history. Public bundles belong in a separate
Git repository; the engine repository owns their consumption contracts.

### Rejected: npm-style package distribution

Publish registries as versioned packages to a package registry (npm or an
npm-shaped index), resolved by package name and version instead of a Git
ref. This would need a new transport, a new package-index contract, and a new
install/update flow parallel to the existing language-pack installer, without
reusing any of the Git fetch, cache, or lock plumbing `wax` already has for
Git-backed registries. It also ties publication to a third-party registry
account instead of the design system's own source repository.

### Selected: separate Git repository with versioned system subfolders

Public bundles use a separate ordinary Git repository, with independent
system/revision subfolders such as `systems/acme/1`. The same source contract
also works for bundles published from existing design-system repositories.

Reuse the existing Git registry contract and extend it with an optional
`subdir` and a `system` id selecting a publisher definition. The definition and
every declared language registry are read as ordinary file blobs at one
resolved, immutable commit — no new transport, no checkout, no submodules,
and no upstream build/publish step. A design system publishes simply by
including `.wax/wax.config.json` and `.wax/registries/<language>.json` files
in its bundle directory and tagging a commit.

## Selected contract summary

- Omitting `subdir` keeps the legacy root Git registry behavior unchanged:
  `.wax/registries/<language>.json` at the resolved commit.
- `subdir` without `system` pins a single conventional registry rooted
  at that subfolder, mirroring the root contract one level down.
- `subdir` with `system` selects exactly one `design_systems[system]` entry
  from `<subdir>/.wax/wax.config.json` and resolves every language registry
  it declares, together, at the same commit.
- The **registry catalogue** (`wax-contract::Catalog`) can describe many
  versioned bundle revisions — one positive revision per system bundle, not
  per language — independent of any one Git commit, config schema, or
  lockfile schema. Catalogue entries carry their own schema version
  (`schema_version: 1`) and do not reference their own distributing commit;
  the catalogue is read as a snapshot.
- Lock identity never collides across systems or subdirs: legacy identity
  stays `git:<url>#<ref>`; nested identity is
  `git-system:<sha256(canonical JSON [url, ref, subdir, system-or-null])>`.
- Registry documents remain schema 1, app/publisher config remains schema 2,
  and new lock writers use schema 3 with optional `subdir` and typed
  `definition: { system_id, path, sha256 }`. Lock schemas 1/2 and old global
  state remain readable; absent new fields are omitted on serialization.
- Catalogue schema, system bundle revision, upstream versions, config schema,
  lock schema, and engine SemVer are independent. `minimum_wax_version` is a
  SemVer identifying a compatible engine release. One positive bundle revision
  advances its definition, every language registry, and provenance together.
- Bundle definitions declare conventional relative registry paths. Their HTTP
  or `published_source` overrides are never followed for a Git bundle. All
  blobs must be regular files, and every expected digest is checked before
  selecting a bundle. Definition/provenance bytes are cached by digest.
- Ordinary resolution reuses full commit pins. Nested revision directories
  never advance implicitly, including during `sync --upgrade`; root Git
  sources retain their existing upgrade behavior. Offline validation uses
  only committed config/lock metadata, without global state or network/cache.

## Delivered consumption contracts

- `wax registry find` reports available exact positive revisions and explicit
  coverage/gaps. `wax registry install <id> --version <n>` remembers the entire
  verified bundle without scaffolding an app or installing language packs.
- `wax init --non-interactive --language <id> --design-system <remembered-id>`
  uses the interactive remembered-system setup path. Every requested language
  must exist before app config/lock writes. Pack installation remains separate,
  with existing `--no-install` behavior.
- Public app config records Git repository, **full commit** as `tag`, revision
  `subdir`, and `system`. Each language lock records its registry digest and the
  shared definition/commit. There are no absolute global registry paths.
- `wax sync --design-system <remembered-id>` explicitly adopts a same-system,
  same-repository bundle across all currently bound languages. Missing language,
  unrelated source/system, corrupt bytes, unavailable exact commit and failed
  persistence preserve prior app files. Rollback preserves exact config bytes.
  Other systems stay unchanged. Sync shows old/new revision directories/commits.
- Ordinary sync, best-effort scan sync, nested `sync --upgrade`, refresh and
  global installation cannot select a newer public revision. Root Git tag
  upgrade keeps its existing behavior. `--design-system` conflicts with
  `--upgrade`.
- App-local caches are populated at init/sync. Offline app search/detail and
  warmed scans need no remembered registry state. A clean checkout may fetch
  the exact locked commit directly; metadata-only validate needs no cache,
  global state or network. CI preinstalls locked packs and uses
  `scan --no-auto-install`.
- Configless scan accepts installed ids and explicit `public:<id>@<revision>`
  through shared digest-checked lookup, without app config/lock writes. Existing
  `--offline` uses exact warmed sources; cold lookup never selects latest.

## Coverage, trust and remaining work

Matching retains parser package/candidate/legacy contracts; bundle setup makes
no parser changes. See [pilot coverage and provenance](../../website/docs/guides/registries.md#public-coverage-and-provenance)
for the seven-pilot matrix, unsupported conventions, absent tokens, and
correction/retention policy. Corrected provenance/curation requires a new
revision; retain old index rows, paths and commits. Public metadata proves only
the recorded coverage and source pins, not upstream endorsement or package
release mapping.

The available catalogue at `761243349f9a74a9a70dc20a8877b430241bcae7` contains
shadcn only, rather than the expected six systems/seven registries. The Phase
3b merge SHA, maintenance/staging evidence, remaining pilot artifacts and
cross-repository documentation are not present there. The full seven-pilot
smoke is tracked in Linear. Language-pack distribution is unchanged and
independent of registry distribution.

Engine contributions follow [Wax contributing](../../CONTRIBUTING.md);
registry contributions belong in [wax-registries](https://github.com/Daio-io/wax-registries).
