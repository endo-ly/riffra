import { readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const directory = join(repositoryRoot, 'apps/desktop/src/lib/generated');
const types = readdirSync(directory)
  .filter((file) => file.endsWith('.ts') && file !== 'index.ts')
  .map((file) => file.slice(0, -3))
  .sort();
const barrel = `${types.map((name) => `export type { ${name} } from './${name}';`).join('\n')}\n`;
writeFileSync(join(directory, 'index.ts'), barrel);
