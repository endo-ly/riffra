import { readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const directory = 'src/lib/generated';
const types = readdirSync(directory)
  .filter((file) => file.endsWith('.ts') && file !== 'index.ts')
  .map((file) => file.slice(0, -3))
  .sort();
const barrel = `${types.map((name) => `export type { ${name} } from './${name}';`).join('\n')}\n`;
writeFileSync(join(directory, 'index.ts'), barrel);
