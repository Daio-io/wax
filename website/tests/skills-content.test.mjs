import assert from 'node:assert/strict';
import {readdir, readFile} from 'node:fs/promises';
import test from 'node:test';

const repository = new URL('../../', import.meta.url);
const skillsRoot = new URL('skills/', repository);
const docsRoot = new URL('website/docs/skills/', repository);
const buildRoot = new URL('website/build/docs/skills/', repository);

test('every shipped skill has a discoverable page and safety boundary', async () => {
  const entries = await readdir(skillsRoot, {withFileTypes: true});
  const chooser = await readFile(new URL('index.md', docsRoot), 'utf8');
  const safetyBoundaries = new Set();

  for (const entry of entries.filter((candidate) => candidate.isDirectory())) {
    const skill = await readFile(
      new URL(`${entry.name}/SKILL.md`, skillsRoot),
      'utf8',
    );
    const frontmatterName = skill.match(/^name:\s*(\S+)$/m)?.[1];
    assert.ok(frontmatterName, `${entry.name} needs a frontmatter name`);

    const source = await readFile(
      new URL(`${frontmatterName}.md`, docsRoot),
      'utf8',
    );
    const html = await readFile(
      new URL(`${frontmatterName}/index.html`, buildRoot),
      'utf8',
    );
    assert.match(chooser, new RegExp(`\\b${frontmatterName}\\b`));
    assert.match(html, new RegExp(`\\b${frontmatterName}\\b`));
    assert.match(
      source,
      new RegExp(
        `https://github\\.com/Daio-io/wax/blob/main/skills/${frontmatterName}/SKILL\\.md`,
      ),
    );

    const boundary = source.match(/^Safety boundary:\s*(.+)$/m)?.[1];
    assert.ok(boundary, `${frontmatterName} needs a named safety boundary`);
    assert.ok(
      !safetyBoundaries.has(boundary),
      `${frontmatterName} needs a distinct safety boundary`,
    );
    safetyBoundaries.add(boundary);
  }
});
