// Writes the language data Monaco's language workers use, from the copies of
// vscode-html-languageservice and vscode-css-languageservice in the repository's
// node_modules (Monaco 0.55.1's):
// - html.json: the HTML elements, global attributes and value sets (`webCustomData`), with
//   their descriptions and references, in the data's order;
// - entities.json: the character entities (`htmlEntities`), name to character, in the
//   data's order;
// - css.json: the CSS properties (`webCustomData`) as the hover documents them: the first of
//   each name, with its description, status, browsers, syntax and references.
//
// node app-rs/crates/treeterm-model/src/code_intel/data/generate.mjs [node_modules], the
// repository's node_modules by default.

import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const nodeModules = process.argv[2] || join(here, '../../../../../../node_modules');
const external = join(nodeModules, 'monaco-editor/esm/external');
const html = join(external, 'vscode-html-languageservice/lib/esm');
const css = join(external, 'vscode-css-languageservice/lib/esm');

const { htmlData } = await import(join(html, 'languageFacts/data/webCustomData.js'));
const { entities } = await import(join(html, 'parser/htmlEntities.js'));
const { cssData } = await import(join(css, 'data/webCustomData.js'));

/** A description as the language service normalizes it (`normalizeMarkupContent`): markdown. */
const description = (input) => (input === undefined ? undefined : typeof input === 'string' ? input : input.value);
const references = (input) => (input || []).map(({ name, url }) => ({ name, url }));
const value = (input) => ({ name: input.name, description: description(input.description), references: references(input.references) });
const attribute = (input) => ({
  name: input.name,
  description: description(input.description),
  valueSet: input.valueSet,
  values: (input.values || []).map(value),
  references: references(input.references),
});

const data = {
  tags: htmlData.tags.map((tag) => ({
    name: tag.name,
    void: tag.void === true,
    description: description(tag.description),
    attributes: tag.attributes.map(attribute),
    references: references(tag.references),
  })),
  globalAttributes: htmlData.globalAttributes.map(attribute),
  valueSets: htmlData.valueSets.map((set) => ({ name: set.name, values: set.values.map(value) })),
};
writeFileSync(join(here, 'html.json'), JSON.stringify(data) + '\n');
writeFileSync(join(here, 'entities.json'), JSON.stringify(Object.entries(entities)) + '\n');

const seen = new Set();
const properties = cssData.properties
  .filter((property) => !seen.has(property.name) && seen.add(property.name))
  .map((property) => ({
    name: property.name,
    description: property.description,
    status: property.status,
    browsers: property.browsers || [],
    syntax: property.syntax || undefined,
    references: references(property.references),
  }));
writeFileSync(join(here, 'css.json'), JSON.stringify({ properties }) + '\n');
