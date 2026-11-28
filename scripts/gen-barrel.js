import { readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const directories = [
  join(repositoryRoot, 'apps/desktop/src/model/generated'),
  join(repositoryRoot, 'packages/visual-renderer/src/generated'),
];
for (const directory of directories) {
  const types = readdirSync(directory)
    .filter((file) => file.endsWith('.ts') && file !== 'index.ts')
    .map((file) => file.slice(0, -3))
    .sort();
  const barrel = `${types.map((name) => `export type { ${name} } from './${name}';`).join('\n')}\n`;
  writeFileSync(join(directory, 'index.ts'), barrel);
}
