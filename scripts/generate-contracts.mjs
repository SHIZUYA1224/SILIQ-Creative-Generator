import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { compile } from 'json-schema-to-typescript';

const schema = JSON.parse(await readFile(new URL('../schemas/design.core.schema.json', import.meta.url), 'utf8'));
schema.title = 'DesignDocument';
const directory = new URL('../packages/contracts-generated/', import.meta.url);
await mkdir(directory, { recursive: true });
await writeFile(new URL('design.ts', directory), await compile(schema, 'DesignDocument', {
  bannerComment: '/* Generated from schemas/design.core.schema.json. Run npm run contracts. */',
  unreachableDefinitions: true,
  ignoreMinAndMaxItems: true,
}));
