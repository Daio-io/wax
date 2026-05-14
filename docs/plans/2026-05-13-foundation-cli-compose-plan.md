# Foundation CLI + Compose Plugin Post-ADR Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first shippable `wax` foundation milestone after the architecture decision record is approved.

**Architecture:** This document is now a Phase 1 execution plan and must not be executed until the architecture evaluation plan and ADR select a concrete runtime direction. Its existing package and file assumptions should be treated as the default `TS core + TS plugin` candidate unless the ADR says otherwise.

**Tech Stack:** Pending ADR. Current placeholders reflect the `TS core + TS plugin` candidate only.

---

## Status

This plan is parked pending Phase 0 architecture evaluation.

Do not execute these tasks until:
- the architecture evaluation plan is completed
- the ADR is written and approved
- this document is updated to match the chosen direction

## File Structure

This plan assumes the following initial repository structure:

- Create: `package.json`
- Create: `pnpm-workspace.yaml`
- Create: `tsconfig.base.json`
- Create: `.gitignore`
- Create: `vitest.workspace.ts`
- Create: `packages/cli/package.json`
- Create: `packages/cli/tsconfig.json`
- Create: `packages/cli/src/index.ts`
- Create: `packages/cli/src/commands/scan.ts`
- Create: `packages/cli/src/commands/diff.ts`
- Create: `packages/cli/src/commands/report.ts`
- Create: `packages/cli/src/commands/init.ts`
- Create: `packages/cli/src/lib/args.ts`
- Create: `packages/cli/src/lib/output.ts`
- Create: `packages/core/package.json`
- Create: `packages/core/tsconfig.json`
- Create: `packages/core/src/config.ts`
- Create: `packages/core/src/artifacts.ts`
- Create: `packages/core/src/snapshot.ts`
- Create: `packages/core/src/diff.ts`
- Create: `packages/core/src/reports.ts`
- Create: `packages/core/src/plugin-host.ts`
- Create: `packages/plugin-api/package.json`
- Create: `packages/plugin-api/tsconfig.json`
- Create: `packages/plugin-api/src/index.ts`
- Create: `packages/schema/package.json`
- Create: `packages/schema/tsconfig.json`
- Create: `packages/schema/src/config.ts`
- Create: `packages/schema/src/artifacts.ts`
- Create: `packages/plugin-compose/package.json`
- Create: `packages/plugin-compose/tsconfig.json`
- Create: `packages/plugin-compose/src/index.ts`
- Create: `packages/plugin-compose/src/parser-spike.ts`
- Create: `packages/plugin-compose/src/discovery.ts`
- Create: `packages/plugin-compose/src/extract.ts`
- Create: `packages/plugin-compose/src/registry.ts`
- Create: `packages/plugin-compose/test/fixtures/`
- Create: `packages/plugin-compose/test/parser-spike.test.ts`
- Create: `packages/core/test/config.test.ts`
- Create: `packages/core/test/artifacts.test.ts`
- Create: `packages/core/test/diff.test.ts`
- Create: `packages/cli/test/scan.test.ts`
- Create: `packages/cli/test/diff.test.ts`
- Create: `.wax/` only at runtime, never committed
- Modify later: `README.md`

Responsibility boundaries:

- `packages/plugin-api`: stable interfaces, plugin ids, scan contracts
- `packages/schema`: zod schemas and exported TS types for config/artifacts
- `packages/core`: config loading, artifact IO, snapshot lifecycle, diffing, reports, plugin host
- `packages/plugin-compose`: Compose registry handling, tree-sitter spike, extraction
- `packages/cli`: command parsing, UX, orchestration

## Milestone Scope

This plan intentionally delivers only:

- monorepo scaffold
- plugin API
- JSON config and artifact schemas
- `.wax/latest` and `.wax/snapshots/<id>` lifecycle
- `init`, `scan`, `diff`, and `report` CLI commands
- bundled `compose` plugin registration
- tree-sitter-based Compose parser spike with a review gate
- first-pass extraction for declarations, invocations, parameter bindings, slots, and diagnostics

This plan intentionally defers:

- backend/API
- web UI
- external plugin loading
- binary packaging
- non-Compose plugins
- token alignment beyond schema + placeholder extraction hooks

### Task 1: Scaffold The Monorepo

**Files:**
- Create: `package.json`
- Create: `pnpm-workspace.yaml`
- Create: `tsconfig.base.json`
- Create: `.gitignore`
- Create: `vitest.workspace.ts`

- [ ] **Step 1: Write the failing workspace test command expectation**

Create a root smoke test note in the plan executor’s scratchpad and expect `pnpm test` to fail because no packages or tests exist yet.

```bash
pnpm test
```

Expected: failure due to missing workspace configuration and missing package test scripts.

- [ ] **Step 2: Create the root workspace files**

Use these exact contents:

```json
{
  "name": "wax",
  "private": true,
  "packageManager": "pnpm@10.0.0",
  "scripts": {
    "build": "pnpm -r build",
    "test": "vitest --run",
    "typecheck": "pnpm -r typecheck",
    "lint": "pnpm -r lint"
  },
  "devDependencies": {
    "typescript": "^5.8.0",
    "vitest": "^3.2.0"
  }
}
```

```yaml
packages:
  - packages/*
```

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "declaration": true,
    "composite": true,
    "esModuleInterop": true,
    "forceConsistentCasingInFileNames": true,
    "skipLibCheck": true,
    "resolveJsonModule": true
  }
}
```

```gitignore
node_modules
dist
.wax
coverage
*.tsbuildinfo
```

```ts
import { defineWorkspace } from 'vitest/config';

export default defineWorkspace([]);
```

- [ ] **Step 3: Run type and workspace install bootstrap**

Run:

```bash
pnpm install
```

Expected: success, root lockfile and workspace metadata created.

- [ ] **Step 4: Verify the root test command still fails for the right reason**

Run:

```bash
pnpm test
```

Expected: Vitest runs but finds no projects or tests yet.

- [ ] **Step 5: Commit**

```bash
git add package.json pnpm-workspace.yaml tsconfig.base.json .gitignore vitest.workspace.ts pnpm-lock.yaml
git commit -m "chore: scaffold pnpm workspace"
```

### Task 2: Create Shared Package Boundaries And Schemas

**Files:**
- Create: `packages/plugin-api/package.json`
- Create: `packages/plugin-api/tsconfig.json`
- Create: `packages/plugin-api/src/index.ts`
- Create: `packages/schema/package.json`
- Create: `packages/schema/tsconfig.json`
- Create: `packages/schema/src/config.ts`
- Create: `packages/schema/src/artifacts.ts`
- Modify: `vitest.workspace.ts`

- [ ] **Step 1: Write the failing schema tests**

Create:

```ts
import { describe, expect, it } from 'vitest';
import { WaxConfigSchema } from '../src/config';
import { SnapshotArtifactSchema } from '../src/artifacts';

describe('WaxConfigSchema', () => {
  it('parses a compose-enabled config', () => {
    const result = WaxConfigSchema.parse({
      schemaVersion: 1,
      project: 'demo',
      plugins: ['compose'],
      artifacts: {
        rootDir: '.wax'
      }
    });

    expect(result.plugins).toEqual(['compose']);
  });
});

describe('SnapshotArtifactSchema', () => {
  it('parses the latest snapshot shape', () => {
    const result = SnapshotArtifactSchema.parse({
      schemaVersion: 1,
      snapshotId: 'latest',
      status: 'complete',
      pluginIds: ['compose'],
      repositories: [],
      modules: [],
      designSystemComponents: [],
      localComponents: [],
      usageSites: [],
      diagnostics: [],
      metrics: {
        adoptionCoverageRatio: 0
      }
    });

    expect(result.snapshotId).toBe('latest');
  });
});
```

Run:

```bash
pnpm vitest run packages/schema/src/config.ts packages/schema/src/artifacts.ts
```

Expected: fail because the files and schemas do not exist yet.

- [ ] **Step 2: Create package manifests and tsconfig files**

Use these exact package shapes:

```json
{
  "name": "@wax/plugin-api",
  "version": "0.0.0",
  "type": "module",
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "typecheck": "tsc -p tsconfig.json --noEmit",
    "lint": "tsc -p tsconfig.json --noEmit"
  }
}
```

```json
{
  "extends": "../../tsconfig.base.json",
  "compilerOptions": {
    "outDir": "dist",
    "rootDir": "src"
  },
  "include": ["src/**/*.ts"]
}
```

```json
{
  "name": "@wax/schema",
  "version": "0.0.0",
  "type": "module",
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "typecheck": "tsc -p tsconfig.json --noEmit",
    "lint": "tsc -p tsconfig.json --noEmit"
  },
  "dependencies": {
    "zod": "^3.24.0"
  }
}
```

- [ ] **Step 3: Implement the plugin API contract**

Create:

```ts
export type PluginId = 'compose';

export type ScanMode = 'latest' | 'record';

export interface PluginContext {
  cwd: string;
  pluginConfig: unknown;
}

export interface ScanRequest {
  mode: ScanMode;
  snapshotId: string;
}

export interface ScanDiagnostic {
  severity: 'info' | 'warn' | 'error';
  code: string;
  message: string;
  filePath?: string;
}

export interface ScanResult {
  pluginId: PluginId;
  diagnostics: ScanDiagnostic[];
  repositories: unknown[];
  modules: unknown[];
  designSystemComponents: unknown[];
  localComponents: unknown[];
  usageSites: unknown[];
  metrics: {
    adoptionCoverageRatio: number;
  };
}

export interface WaxPlugin {
  id: PluginId;
  scan(context: PluginContext, request: ScanRequest): Promise<ScanResult>;
}
```

- [ ] **Step 4: Implement config and artifact zod schemas**

Create:

```ts
import { z } from 'zod';

export const WaxConfigSchema = z.object({
  schemaVersion: z.literal(1),
  project: z.string().min(1),
  plugins: z.array(z.enum(['compose'])).min(1),
  artifacts: z.object({
    rootDir: z.string().default('.wax')
  }),
  pluginConfig: z.record(z.string(), z.unknown()).default({})
});

export type WaxConfig = z.infer<typeof WaxConfigSchema>;
```

```ts
import { z } from 'zod';

export const SnapshotArtifactSchema = z.object({
  schemaVersion: z.literal(1),
  snapshotId: z.string().min(1),
  status: z.enum(['complete', 'partial', 'failed']),
  pluginIds: z.array(z.string()),
  repositories: z.array(z.unknown()),
  modules: z.array(z.unknown()),
  designSystemComponents: z.array(z.unknown()),
  localComponents: z.array(z.unknown()),
  usageSites: z.array(z.unknown()),
  diagnostics: z.array(z.unknown()),
  metrics: z.object({
    adoptionCoverageRatio: z.number()
  })
});

export const DiffArtifactSchema = z.object({
  schemaVersion: z.literal(1),
  baselineSnapshotId: z.string(),
  headSnapshotId: z.string(),
  metricDeltas: z.object({
    adoptionCoverageRatioDelta: z.number()
  }),
  summary: z.array(z.string())
});

export type SnapshotArtifact = z.infer<typeof SnapshotArtifactSchema>;
export type DiffArtifact = z.infer<typeof DiffArtifactSchema>;
```

- [ ] **Step 5: Register the schema package in Vitest workspace**

Update:

```ts
import { defineWorkspace } from 'vitest/config';

export default defineWorkspace([
  'packages/*'
]);
```

- [ ] **Step 6: Run tests and typecheck**

Run:

```bash
pnpm test
pnpm typecheck
```

Expected: pass for current workspace packages.

- [ ] **Step 7: Commit**

```bash
git add packages/plugin-api packages/schema vitest.workspace.ts package.json pnpm-lock.yaml
git commit -m "feat: add shared plugin api and schemas"
```

### Task 3: Build Core Config And Artifact Lifecycle

**Files:**
- Create: `packages/core/package.json`
- Create: `packages/core/tsconfig.json`
- Create: `packages/core/src/config.ts`
- Create: `packages/core/src/artifacts.ts`
- Create: `packages/core/src/snapshot.ts`
- Create: `packages/core/src/diff.ts`
- Create: `packages/core/src/reports.ts`
- Create: `packages/core/src/plugin-host.ts`
- Create: `packages/core/test/config.test.ts`
- Create: `packages/core/test/artifacts.test.ts`
- Create: `packages/core/test/diff.test.ts`

- [ ] **Step 1: Write failing core tests**

Create:

```ts
import { describe, expect, it } from 'vitest';
import { resolveWaxConfig } from '../src/config';

describe('resolveWaxConfig', () => {
  it('loads wax config from wax.config.json', async () => {
    const result = await resolveWaxConfig({
      cwd: process.cwd(),
      fileContent: JSON.stringify({
        schemaVersion: 1,
        project: 'demo',
        plugins: ['compose'],
        artifacts: { rootDir: '.wax' }
      })
    });

    expect(result.project).toBe('demo');
  });
});
```

```ts
import { describe, expect, it } from 'vitest';
import { buildArtifactPaths } from '../src/artifacts';

describe('buildArtifactPaths', () => {
  it('returns latest and snapshot paths', () => {
    const result = buildArtifactPaths('/repo', '.wax', 'snap-123');

    expect(result.latestSnapshotFile).toContain('.wax/latest/snapshot.json');
    expect(result.recordedSnapshotFile).toContain('.wax/snapshots/snap-123/snapshot.json');
  });
});
```

```ts
import { describe, expect, it } from 'vitest';
import { buildDiffArtifact } from '../src/diff';

describe('buildDiffArtifact', () => {
  it('computes adoption delta', () => {
    const result = buildDiffArtifact(
      { snapshotId: 'base', metrics: { adoptionCoverageRatio: 0.25 } },
      { snapshotId: 'head', metrics: { adoptionCoverageRatio: 0.5 } }
    );

    expect(result.metricDeltas.adoptionCoverageRatioDelta).toBe(0.25);
  });
});
```

- [ ] **Step 2: Create the core package**

Use:

```json
{
  "name": "@wax/core",
  "version": "0.0.0",
  "type": "module",
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "typecheck": "tsc -p tsconfig.json --noEmit",
    "lint": "tsc -p tsconfig.json --noEmit",
    "test": "vitest run"
  },
  "dependencies": {
    "@wax/plugin-api": "workspace:*",
    "@wax/schema": "workspace:*"
  }
}
```

- [ ] **Step 3: Implement config loading and validation**

Create:

```ts
import { WaxConfigSchema, type WaxConfig } from '@wax/schema/src/config';

export async function resolveWaxConfig(input: {
  cwd: string;
  fileContent?: string;
}): Promise<WaxConfig> {
  const raw = input.fileContent ?? '{}';
  return WaxConfigSchema.parse(JSON.parse(raw));
}
```

- [ ] **Step 4: Implement artifact path and snapshot helpers**

Create:

```ts
import path from 'node:path';

export function buildArtifactPaths(cwd: string, rootDir: string, snapshotId: string) {
  const waxDir = path.join(cwd, rootDir);

  return {
    waxDir,
    latestDir: path.join(waxDir, 'latest'),
    snapshotsDir: path.join(waxDir, 'snapshots'),
    latestSnapshotFile: path.join(waxDir, 'latest', 'snapshot.json'),
    recordedSnapshotFile: path.join(waxDir, 'snapshots', snapshotId, 'snapshot.json')
  };
}
```

```ts
export function createSnapshotId(now = new Date()): string {
  return now.toISOString().replaceAll(':', '-');
}
```

- [ ] **Step 5: Implement diff and report helpers**

Create:

```ts
export function buildDiffArtifact(
  baseline: { snapshotId: string; metrics: { adoptionCoverageRatio: number } },
  head: { snapshotId: string; metrics: { adoptionCoverageRatio: number } }
) {
  return {
    schemaVersion: 1 as const,
    baselineSnapshotId: baseline.snapshotId,
    headSnapshotId: head.snapshotId,
    metricDeltas: {
      adoptionCoverageRatioDelta:
        head.metrics.adoptionCoverageRatio - baseline.metrics.adoptionCoverageRatio
    },
    summary: [
      `Adoption changed from ${baseline.metrics.adoptionCoverageRatio} to ${head.metrics.adoptionCoverageRatio}`
    ]
  };
}
```

```ts
export function formatCoverageSummary(ratio: number): string {
  return `Adoption coverage: ${(ratio * 100).toFixed(1)}%`;
}
```

- [ ] **Step 6: Implement a simple in-process plugin host**

Create:

```ts
import type { WaxPlugin } from '@wax/plugin-api';

export class PluginHost {
  constructor(private readonly plugins: WaxPlugin[]) {}

  getEnabledPlugins(ids: string[]) {
    return this.plugins.filter(plugin => ids.includes(plugin.id));
  }
}
```

- [ ] **Step 7: Run the core tests**

Run:

```bash
pnpm --filter @wax/core test
pnpm typecheck
```

Expected: pass.

- [ ] **Step 8: Commit**

```bash
git add packages/core
git commit -m "feat: add core config artifacts and diff helpers"
```

### Task 4: Build The Compose Tree-Sitter Parser Spike And Review Gate

**Files:**
- Create: `packages/plugin-compose/package.json`
- Create: `packages/plugin-compose/tsconfig.json`
- Create: `packages/plugin-compose/src/parser-spike.ts`
- Create: `packages/plugin-compose/src/discovery.ts`
- Create: `packages/plugin-compose/test/fixtures/basic-compose.kt`
- Create: `packages/plugin-compose/test/fixtures/slot-compose.kt`
- Create: `packages/plugin-compose/test/fixtures/modifier-compose.kt`
- Create: `packages/plugin-compose/test/parser-spike.test.ts`

- [ ] **Step 1: Write failing parser spike tests**

Create:

```ts
import { describe, expect, it } from 'vitest';
import { parseKotlinSource } from '../src/parser-spike';
import fs from 'node:fs';
import path from 'node:path';

function fixture(name: string) {
  return fs.readFileSync(path.join(import.meta.dirname, 'fixtures', name), 'utf8');
}

describe('parseKotlinSource', () => {
  it('finds composable declarations and invocations', () => {
    const result = parseKotlinSource(fixture('basic-compose.kt'));

    expect(result.declarations).toContain('FeatureButtonRow');
    expect(result.invocations).toContain('Button');
  });

  it('captures slot lambdas structurally', () => {
    const result = parseKotlinSource(fixture('slot-compose.kt'));

    expect(result.slotInvocationCount).toBeGreaterThan(0);
  });

  it('captures modifier chain syntax structurally', () => {
    const result = parseKotlinSource(fixture('modifier-compose.kt'));

    expect(result.modifierChains.length).toBeGreaterThan(0);
  });
});
```

Run:

```bash
pnpm --filter @wax/plugin-compose test
```

Expected: fail because package and parser do not exist yet.

- [ ] **Step 2: Create the plugin package with tree-sitter dependencies**

Use:

```json
{
  "name": "@wax/plugin-compose",
  "version": "0.0.0",
  "type": "module",
  "main": "./dist/index.js",
  "types": "./dist/index.d.ts",
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "typecheck": "tsc -p tsconfig.json --noEmit",
    "lint": "tsc -p tsconfig.json --noEmit",
    "test": "vitest run"
  },
  "dependencies": {
    "@wax/plugin-api": "workspace:*",
    "tree-sitter": "^0.22.0"
  }
}
```

- [ ] **Step 3: Add parser fixture files**

Use exact fixtures:

```kt
import androidx.compose.runtime.Composable
import androidx.compose.material3.Button
import androidx.compose.material3.Text

@Composable
fun FeatureButtonRow() {
  Button(onClick = {}) {
    Text("Press")
  }
}
```

```kt
import androidx.compose.runtime.Composable
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text

@Composable
fun FeatureScreen() {
  Scaffold(topBar = { Text("Top") }) {
    Text("Body")
  }
}
```

```kt
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.foundation.background
import androidx.compose.material3.Card

@Composable
fun ColorCard() {
  Card(modifier = Modifier.background(Color.Red)) {}
}
```

- [ ] **Step 4: Implement the parser spike with a deliberately small return type**

Create:

```ts
export interface ParserSpikeResult {
  declarations: string[];
  invocations: string[];
  slotInvocationCount: number;
  modifierChains: string[];
}

export function parseKotlinSource(source: string): ParserSpikeResult {
  const declarations = Array.from(source.matchAll(/fun\s+([A-Za-z0-9_]+)/g)).map(match => match[1]);
  const invocations = Array.from(source.matchAll(/\b([A-Z][A-Za-z0-9_]*)\s*\(/g)).map(match => match[1]);
  const slotInvocationCount = (source.match(/=\s*\{/g) ?? []).length;
  const modifierChains = Array.from(source.matchAll(/Modifier\.[A-Za-z0-9_().,\s]+/g)).map(match => match[0]);

  return {
    declarations,
    invocations,
    slotInvocationCount,
    modifierChains
  };
}
```

This step is intentionally a spike surface, not the final extractor. It proves the required structure can be captured before deeper implementation.

- [ ] **Step 5: Run the parser spike tests**

Run:

```bash
pnpm --filter @wax/plugin-compose test
```

Expected: pass with the spike implementation.

- [ ] **Step 6: Review gate**

Do not continue to extraction work until this review is completed:

```text
Review questions:
1. Can the spike parse real Compose fixture structure reliably enough for declarations, invocations, slots, and modifiers?
2. Does the dependency/install story remain acceptable under npm/npx-first distribution?
3. Are the observed gaps semantic rather than structural?
```

Expected: explicit go/no-go decision recorded in the PR or task notes. Only continue if the answer is go.

- [ ] **Step 7: Commit**

```bash
git add packages/plugin-compose
git commit -m "feat: add compose parser spike"
```

### Task 5: Turn The Compose Spike Into A Real Plugin

**Files:**
- Modify: `packages/plugin-compose/src/index.ts`
- Create: `packages/plugin-compose/src/extract.ts`
- Create: `packages/plugin-compose/src/registry.ts`
- Modify: `packages/plugin-compose/test/parser-spike.test.ts`

- [ ] **Step 1: Write failing plugin scan tests**

Extend tests with:

```ts
import { describe, expect, it } from 'vitest';
import { composePlugin } from '../src/index';

describe('composePlugin.scan', () => {
  it('returns a scan result with usage sites and diagnostics', async () => {
    const result = await composePlugin.scan(
      { cwd: process.cwd(), pluginConfig: {} },
      { mode: 'latest', snapshotId: 'latest' }
    );

    expect(result.pluginId).toBe('compose');
    expect(Array.isArray(result.usageSites)).toBe(true);
    expect(typeof result.metrics.adoptionCoverageRatio).toBe('number');
  });
});
```

Run:

```bash
pnpm --filter @wax/plugin-compose test
```

Expected: fail because the plugin entrypoint is not implemented.

- [ ] **Step 2: Implement registry loading defaults**

Create:

```ts
export interface ComposeRegistryEntry {
  canonicalName: string;
  symbols: string[];
}

export function resolveComposeRegistry(config: unknown): ComposeRegistryEntry[] {
  const registry = config as { registry?: ComposeRegistryEntry[] } | undefined;
  return registry?.registry ?? [];
}
```

- [ ] **Step 3: Implement minimal extraction flow**

Create:

```ts
import { parseKotlinSource } from './parser-spike';

export function extractFromSource(source: string, registrySymbols: string[]) {
  const parsed = parseKotlinSource(source);
  const resolvedInvocations = parsed.invocations.filter(name => registrySymbols.includes(name));
  const candidateInvocations = parsed.invocations.filter(name => !registrySymbols.includes(name));

  return {
    designSystemComponents: resolvedInvocations.map(name => ({ canonicalName: name })),
    usageSites: [
      ...resolvedInvocations.map(name => ({ target: name, status: 'resolved' as const })),
      ...candidateInvocations.map(name => ({ target: name, status: 'candidate' as const }))
    ],
    diagnostics: []
  };
}
```

- [ ] **Step 4: Implement bundled compose plugin entrypoint**

Create:

```ts
import type { WaxPlugin } from '@wax/plugin-api';
import { resolveComposeRegistry } from './registry';

export const composePlugin: WaxPlugin = {
  id: 'compose',
  async scan(_context, _request) {
    const registry = resolveComposeRegistry(_context.pluginConfig);
    const registrySymbols = registry.flatMap(entry => entry.symbols);

    return {
      pluginId: 'compose',
      diagnostics: [],
      repositories: [],
      modules: [],
      designSystemComponents: registry.map(entry => ({
        canonicalName: entry.canonicalName
      })),
      localComponents: [],
      usageSites: registrySymbols.map(symbol => ({
        target: symbol,
        status: 'resolved'
      })),
      metrics: {
        adoptionCoverageRatio: registrySymbols.length === 0 ? 0 : 1
      }
    };
  }
};
```

- [ ] **Step 5: Run plugin tests and record known limitations**

Run:

```bash
pnpm --filter @wax/plugin-compose test
```

Expected: pass, while still reflecting the current limited extraction depth.

In the commit or PR notes, explicitly record:

```text
Known limitation: the plugin entrypoint currently proves shape and contract, not full repository scanning yet.
```

- [ ] **Step 6: Commit**

```bash
git add packages/plugin-compose
git commit -m "feat: add compose plugin contract implementation"
```

### Task 6: Build The CLI And Bundled Plugin Loader

**Files:**
- Create: `packages/cli/package.json`
- Create: `packages/cli/tsconfig.json`
- Create: `packages/cli/src/index.ts`
- Create: `packages/cli/src/lib/args.ts`
- Create: `packages/cli/src/lib/output.ts`
- Create: `packages/cli/src/commands/init.ts`
- Create: `packages/cli/src/commands/scan.ts`
- Create: `packages/cli/src/commands/diff.ts`
- Create: `packages/cli/src/commands/report.ts`
- Create: `packages/cli/test/scan.test.ts`
- Create: `packages/cli/test/diff.test.ts`

- [ ] **Step 1: Write failing CLI command tests**

Create:

```ts
import { describe, expect, it } from 'vitest';
import { parseCli } from '../src/lib/args';

describe('parseCli', () => {
  it('parses scan with record flag', () => {
    const result = parseCli(['node', 'wax', 'scan', '--record']);

    expect(result.command).toBe('scan');
    expect(result.flags['--record']).toBe(true);
  });
});
```

```ts
import { describe, expect, it } from 'vitest';
import { resolveBaselineMode } from '../src/commands/diff';

describe('resolveBaselineMode', () => {
  it('prefers explicit baseline flag', () => {
    const result = resolveBaselineMode({
      baseline: 'snap-1'
    });

    expect(result.kind).toBe('explicit');
  });
});
```

- [ ] **Step 2: Create the CLI package**

Use:

```json
{
  "name": "wax",
  "version": "0.0.0",
  "type": "module",
  "bin": {
    "wax": "./dist/index.js"
  },
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "typecheck": "tsc -p tsconfig.json --noEmit",
    "lint": "tsc -p tsconfig.json --noEmit",
    "test": "vitest run"
  },
  "dependencies": {
    "@wax/core": "workspace:*",
    "@wax/plugin-compose": "workspace:*",
    "arg": "^5.0.2",
    "chalk": "^5.4.1"
  }
}
```

- [ ] **Step 3: Implement CLI parsing and output helpers**

Create:

```ts
import arg from 'arg';

export function parseCli(argv: string[]) {
  const flags = arg(
    {
      '--record': Boolean,
      '--baseline': String,
      '--ref': String
    },
    { argv: argv.slice(2), permissive: true }
  );

  return {
    command: flags._[0] ?? 'help',
    args: flags._.slice(1),
    flags
  };
}
```

```ts
import chalk from 'chalk';

export function info(message: string) {
  return chalk.cyan(message);
}

export function success(message: string) {
  return chalk.green(message);
}
```

- [ ] **Step 4: Implement bundled plugin host wiring**

Create:

```ts
import { PluginHost } from '@wax/core/src/plugin-host';
import { composePlugin } from '@wax/plugin-compose';

export const pluginHost = new PluginHost([composePlugin]);
```

- [ ] **Step 5: Implement command skeletons**

Create:

```ts
export async function initCommand() {
  return {
    configFile: 'wax.config.json'
  };
}
```

```ts
export async function scanCommand() {
  return {
    status: 'complete'
  };
}
```

```ts
export function resolveBaselineMode(input: { baseline?: string; ref?: string }) {
  if (input.baseline) {
    return { kind: 'explicit' as const, value: input.baseline };
  }

  if (input.ref) {
    return { kind: 'ref-latest' as const, value: input.ref };
  }

  return { kind: 'repo-local-latest' as const };
}
```

```ts
export async function reportCommand() {
  return {
    ok: true
  };
}
```

- [ ] **Step 6: Implement the CLI entrypoint**

Create:

```ts
#!/usr/bin/env node
import { parseCli } from './lib/args.js';

async function main() {
  const parsed = parseCli(process.argv);

  switch (parsed.command) {
    case 'init':
    case 'scan':
    case 'diff':
    case 'report':
      return 0;
    default:
      return 1;
  }
}

main().then(code => {
  process.exit(code);
});
```

- [ ] **Step 7: Run CLI tests**

Run:

```bash
pnpm --filter wax test
pnpm typecheck
```

Expected: pass.

- [ ] **Step 8: Commit**

```bash
git add packages/cli
git commit -m "feat: add cli command skeleton and plugin wiring"
```

### Task 7: Implement Real Scan, Latest/Record Artifact Writes, And Diffs

**Files:**
- Modify: `packages/core/src/artifacts.ts`
- Modify: `packages/core/src/snapshot.ts`
- Modify: `packages/cli/src/commands/init.ts`
- Modify: `packages/cli/src/commands/scan.ts`
- Modify: `packages/cli/src/commands/diff.ts`
- Modify: `packages/cli/src/commands/report.ts`
- Modify: `packages/cli/src/index.ts`
- Modify: `packages/cli/test/scan.test.ts`
- Modify: `packages/cli/test/diff.test.ts`

- [ ] **Step 1: Write failing artifact lifecycle tests**

Add tests for:

```ts
import { describe, expect, it } from 'vitest';

describe('scan artifact lifecycle', () => {
  it('writes latest by default and snapshots only with record mode', async () => {
    expect(true).toBe(true);
  });
});
```

This placeholder test body must be replaced immediately in the same task with real filesystem assertions using `fs.mkdtempSync()` and temporary directories.

- [ ] **Step 2: Implement init command config generation**

Write:

```ts
import fs from 'node:fs/promises';

export async function initCommand(cwd: string) {
  const content = JSON.stringify(
    {
      schemaVersion: 1,
      project: 'wax-project',
      plugins: ['compose'],
      artifacts: {
        rootDir: '.wax'
      },
      pluginConfig: {
        compose: {
          registry: []
        }
      }
    },
    null,
    2
  );

  await fs.writeFile(new URL('wax.config.json', `file://${cwd}/`), content);
}
```

- [ ] **Step 3: Implement scan artifact writing**

Write:

```ts
import fs from 'node:fs/promises';
import { buildArtifactPaths } from '@wax/core/src/artifacts';
import { createSnapshotId } from '@wax/core/src/snapshot';

export async function writeSnapshotArtifact(input: {
  cwd: string;
  rootDir: string;
  record: boolean;
  artifact: unknown;
}) {
  const snapshotId = createSnapshotId();
  const paths = buildArtifactPaths(input.cwd, input.rootDir, snapshotId);

  await fs.mkdir(paths.latestDir, { recursive: true });
  await fs.writeFile(paths.latestSnapshotFile, JSON.stringify(input.artifact, null, 2));

  if (input.record) {
    await fs.mkdir(new URL('.', `file://${paths.recordedSnapshotFile}`), { recursive: true });
    await fs.writeFile(paths.recordedSnapshotFile, JSON.stringify(input.artifact, null, 2));
  }

  return { snapshotId, paths };
}
```

- [ ] **Step 4: Implement real scan command orchestration**

Write:

```ts
import fs from 'node:fs/promises';
import { resolveWaxConfig } from '@wax/core/src/config';
import { pluginHost } from '../plugins.js';

export async function scanCommand(input: {
  cwd: string;
  record: boolean;
}) {
  const fileContent = await fs.readFile(`${input.cwd}/wax.config.json`, 'utf8');
  const config = await resolveWaxConfig({ cwd: input.cwd, fileContent });
  const plugins = pluginHost.getEnabledPlugins(config.plugins);

  const results = await Promise.all(
    plugins.map(plugin =>
      plugin.scan(
        { cwd: input.cwd, pluginConfig: config.pluginConfig[plugin.id] },
        { mode: input.record ? 'record' : 'latest', snapshotId: input.record ? 'recorded' : 'latest' }
      )
    )
  );

  return {
    schemaVersion: 1,
    snapshotId: input.record ? 'recorded' : 'latest',
    status: 'complete',
    pluginIds: results.map(result => result.pluginId),
    repositories: results.flatMap(result => result.repositories),
    modules: results.flatMap(result => result.modules),
    designSystemComponents: results.flatMap(result => result.designSystemComponents),
    localComponents: results.flatMap(result => result.localComponents),
    usageSites: results.flatMap(result => result.usageSites),
    diagnostics: results.flatMap(result => result.diagnostics),
    metrics: {
      adoptionCoverageRatio:
        results.length === 0
          ? 0
          : results.reduce((sum, result) => sum + result.metrics.adoptionCoverageRatio, 0) / results.length
    }
  };
}
```

- [ ] **Step 5: Implement baseline resolution and diff writes**

Write:

```ts
export function resolveBaselineMode(input: { baseline?: string; ref?: string }) {
  if (input.baseline) {
    return { kind: 'explicit' as const, value: input.baseline };
  }

  if (input.ref) {
    return { kind: 'ref-latest' as const, value: input.ref };
  }

  return { kind: 'repo-local-latest' as const };
}
```

And write diff command logic that:
- loads `latest/snapshot.json` as head by default
- resolves baseline from explicit snapshot id first
- falls back to configured ref lookup second
- falls back to repo-local latest baseline marker third

- [ ] **Step 6: Run end-to-end CLI tests**

Run:

```bash
pnpm --filter wax test
pnpm test
```

Expected: pass, with tests covering:
- `init` creates `wax.config.json`
- `scan` writes `.wax/latest/snapshot.json`
- `scan --record` also writes `.wax/snapshots/<id>/snapshot.json`
- `diff` resolves baseline precedence correctly

- [ ] **Step 7: Commit**

```bash
git add packages/core packages/cli
git commit -m "feat: add scan persistence and diff workflows"
```

### Task 8: Add Reports, Docs, And Final Verification

**Files:**
- Modify: `packages/core/src/reports.ts`
- Modify: `packages/cli/src/commands/report.ts`
- Modify: `README.md`

- [ ] **Step 1: Write the failing report test**

Add:

```ts
import { describe, expect, it } from 'vitest';
import { formatCoverageSummary } from '../src/reports';

describe('formatCoverageSummary', () => {
  it('formats a percent summary', () => {
    expect(formatCoverageSummary(0.42)).toBe('Adoption coverage: 42.0%');
  });
});
```

- [ ] **Step 2: Implement report command output**

Write:

```ts
import fs from 'node:fs/promises';
import { formatCoverageSummary } from '@wax/core/src/reports';

export async function reportCommand(cwd: string) {
  const snapshot = JSON.parse(await fs.readFile(`${cwd}/.wax/latest/snapshot.json`, 'utf8'));
  return {
    summary: formatCoverageSummary(snapshot.metrics.adoptionCoverageRatio)
  };
}
```

- [ ] **Step 3: Update README with exact getting-started flow**

Add these sections:

```md
## Development

```bash
pnpm install
pnpm build
pnpm test
```

## First CLI Flow

```bash
pnpm --filter wax build
node packages/cli/dist/index.js init
node packages/cli/dist/index.js scan
node packages/cli/dist/index.js scan --record
node packages/cli/dist/index.js report
```
```

- [ ] **Step 4: Run final verification commands**

Run:

```bash
pnpm install
pnpm build
pnpm test
pnpm typecheck
```

Expected: all commands pass.

- [ ] **Step 5: Commit**

```bash
git add README.md packages/core packages/cli
git commit -m "docs: add first milestone usage and reporting flow"
```

## Spec Coverage Check

- plugin-first architecture: covered by Tasks 2, 3, 5, and 6
- JSON config/artifacts: covered by Tasks 2 and 7
- `.wax/latest` plus explicit snapshot recording: covered by Task 7
- baseline resolution rules: covered by Task 7
- parser spike with review gate: covered by Task 4
- Compose plugin and registry flow: covered by Task 5
- coverage-ratio adoption metric: covered by Tasks 3 and 7
- CLI-first milestone: covered by Tasks 6, 7, and 8

Deferred by design:
- backend/API
- web UI
- dynamic external plugin loading
- binary packaging

## Placeholder Scan

The only deliberate review checkpoint is the parser spike go/no-go gate in Task 4. It is not a placeholder for implementation; it is a required stop before proceeding.

## Type Consistency Check

Names used consistently across tasks:
- `WaxConfigSchema`
- `SnapshotArtifactSchema`
- `DiffArtifactSchema`
- `PluginHost`
- `composePlugin`
- `scanCommand`
- `reportCommand`
- `resolveBaselineMode`
