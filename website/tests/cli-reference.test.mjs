import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';

const repository = new URL('../../', import.meta.url);
const cliSource = new URL('engine/crates/wax-cli/src/cli.rs', repository);
const cliRoute = new URL('website/build/docs/reference/cli/index.html', repository);

function enumVariants(source, enumName) {
  const match = source.match(
    new RegExp(`pub enum ${enumName} \\{([\\s\\S]*?)\\n\\}`),
  );
  assert.ok(match, `missing ${enumName} in cli.rs`);
  return [...match[1].matchAll(/^ {4}([A-Z][A-Za-z0-9]*)(?:\(|,)/gm)].map(
    ([, variant]) =>
      variant.replace(/([a-z0-9])([A-Z])/g, '$1-$2').toLowerCase(),
  );
}

test('CLI reference covers the complete Clap command surface', async () => {
  const source = await readFile(cliSource, 'utf8');
  const html = await readFile(cliRoute, 'utf8');

  for (const command of enumVariants(source, 'Commands')) {
    assert.match(html, new RegExp(`wax(?:<!-- -->)? ${command}`), command);
  }
  for (const command of enumVariants(source, 'RegistrySubcommand')) {
    assert.match(
      html,
      new RegExp(`wax(?:<!-- -->)? registry(?:<!-- -->)? ${command}`),
      `registry ${command}`,
    );
  }
  for (const command of enumVariants(source, 'LanguageSubcommand')) {
    assert.match(
      html,
      new RegExp(`wax(?:<!-- -->)? language(?:<!-- -->)? ${command}`),
      `language ${command}`,
    );
  }

  for (const required of [
    '--format',
    'summary',
    'quiet',
    'json-summary',
    'markdown',
    'text',
    'json',
    '--kind',
    'component',
    'token',
    '--output',
    'FORMAT=PATH',
    '&lt;id>@&lt;version>',
    '--design-system',
    '--language',
    '--tag',
    '--full',
    'default: 20',
    'compatibility alias',
  ]) {
    assert.match(html, new RegExp(required), required);
  }

  assert.match(html, /wax(?:<!-- -->)? uninstall(?:<!-- -->)? --full/);
  assert.doesNotMatch(html, /wax(?:<!-- -->)? uninstall(?:<!-- -->)? \[--full\]/);
  assert.match(
    html,
    /(?:requires[\s\S]{0,80}--full|--full[\s\S]{0,80}(?:is )?required)/i,
  );
});
