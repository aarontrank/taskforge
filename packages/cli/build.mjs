import { build } from 'esbuild';
import { readFileSync, writeFileSync } from 'fs';

// Strip shebang from tsc-compiled entry so esbuild doesn't see it
// (esbuild adds its own via banner; double shebang causes a syntax error)
const entryPath = 'dist/index.js';
const src = readFileSync(entryPath, 'utf8');
if (src.startsWith('#!')) {
  writeFileSync(entryPath, src.replace(/^#![^\n]*\n/, ''));
}

await build({
  entryPoints: [entryPath],
  bundle: true,
  platform: 'node',
  target: 'node20',
  format: 'cjs',
  outfile: 'dist/bundle.cjs',
  packages: 'bundle',
  external: ['fsevents'],
  banner: { js: '#!/usr/bin/env node' },
});
