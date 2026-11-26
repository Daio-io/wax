# Pinned public catalogue fixtures

`manifest.json` records original paths and SHA-256 digests copied without
modification from `Daio-io/wax-registries` commit
`761243349f9a74a9a70dc20a8877b430241bcae7`. Fixture filenames avoid ignored
`.wax/` directories. The catalogue at that commit contains **only shadcn**;
it does not contain the other six pilot registries expected by THE-894.
Do not substitute synthetic registries for those pinned upstream artifacts.

The shadcn registry has 353 components, covers exact
`@/components/ui/<file>` imports, and declares no tokens. Its provenance pins
upstream `96da9cc7f1aa10580a3741664e2a77c721206c00`. The upstream MIT license
is retained in `shadcn/LICENSE`; the minimal consumer examples are newly
authored MIT examples under the same license terms. Positive, wrong-package,
and unknown-import consumers exercise the public React scanner.

CLI lifecycle tests use a separate local synthetic two-revision Git catalogue
and test packs. They never access public network services. Adding the remaining
pinned pilots is tracked in Linear; see the
[public registry catalogue ADR](../../../docs/adr/2026-11-16-public-registry-catalogue.md).
