import fs from 'node:fs';
import path from 'node:path';
import { parse } from '@babel/parser';

const root = path.resolve(import.meta.dirname, '..');
const languages = ['en', 'zh-CN'];
const namespaces = ['common', 'workspace', 'settings', 'editor', 'errors', 'native'];
const issues = [];
const read = file => JSON.parse(fs.readFileSync(path.join(root, file), 'utf8'));
function flatten(value, prefix = '', result = {}) {
  for (const [key, child] of Object.entries(value)) {
    const full = prefix ? `${prefix}.${key}` : key;
    if (typeof child === 'string') result[full] = child;
    else if (child && typeof child === 'object' && !Array.isArray(child)) flatten(child, full, result);
    else issues.push(`${full}: translation must be a string or namespace object`);
  }
  return result;
}
const variables = value => [...value.matchAll(/{{-?\s*([\w.]+)(?:,[^}]*)?\s*}}/g)].map(match => match[1]).sort().join(',');
const plural = /_(zero|one|two|few|many|other)$/;
for (const ns of namespaces) {
  let baseline;
  for (const language of languages) {
    let catalog;
    try { catalog = flatten(read(`locales/${language}/${ns}.json`)); }
    catch (error) { issues.push(`${language}/${ns}: ${error.message}`); continue; }
    if (!Object.keys(catalog).length) issues.push(`${language}/${ns}: empty catalog`);
    for (const [key, value] of Object.entries(catalog)) if (!value.trim()) issues.push(`${language}/${ns}/${key}: empty translation`);
    const ownPluralBases = new Set(Object.keys(catalog).filter(key => plural.test(key)).map(key => key.replace(plural, '')));
    for (const base of ownPluralBases) {
      for (const category of new Intl.PluralRules(language).resolvedOptions().pluralCategories) {
        if (!(`${base}_${category}` in catalog)) issues.push(`${language}/${ns}/${base}_${category}: missing native plural branch`);
      }
    }
    if (language === 'en') { baseline = catalog; continue; }
    if (!baseline) continue;
    for (const [key, value] of Object.entries(baseline)) {
      if (plural.test(key)) continue;
      if (!(key in catalog)) issues.push(`${language}/${ns}/${key}: missing`);
      else if (variables(value) !== variables(catalog[key])) issues.push(`${language}/${ns}/${key}: interpolation differs`);
    }
    const bases = new Set(Object.keys(baseline).filter(key => plural.test(key)).map(key => key.replace(plural, '')));
    for (const base of bases) {
      for (const category of new Intl.PluralRules(language).resolvedOptions().pluralCategories) {
        const key = `${base}_${category}`;
        if (!(key in catalog)) issues.push(`${language}/${ns}/${key}: missing plural branch`);
        else if (variables(catalog[key]) !== variables(baseline[`${base}_other`] ?? '')) issues.push(`${language}/${ns}/${key}: plural interpolation differs`);
      }
    }
    for (const key of Object.keys(catalog)) {
      if (!(key in baseline) && !(plural.test(key) && bases.has(key.replace(plural, '')))) issues.push(`${language}/${ns}/${key}: no English source`);
    }
  }
}
// Check application UI literals. Test payloads may intentionally contain Chinese.
for (const file of fs.readdirSync(path.join(root, 'src/workspace')).filter(file => /\.tsx?$/.test(file))) {
  const source = parse(fs.readFileSync(path.join(root, 'src/workspace', file), 'utf8'), {
    sourceType: 'module', plugins: file.endsWith('.tsx') ? ['typescript', 'jsx'] : ['typescript'], attachComment: false,
  });
  function visit(node) {
    if (!node || typeof node.type !== 'string') return;
    const text = node.type === 'TemplateElement' ? (node.value.cooked ?? node.value.raw)
      : ['StringLiteral', 'DirectiveLiteral', 'JSXText'].includes(node.type) ? node.value : '';
    if (/\p{Script=Han}/u.test(text)) {
      issues.push(`src/workspace/${file}:${node.loc.start.line}: hardcoded UI text ${JSON.stringify(text.trim().slice(0, 70))}`);
    }
    for (const child of Object.values(node)) {
      if (Array.isArray(child)) child.forEach(visit);
      else if (child && typeof child === 'object') visit(child);
    }
  }
  visit(source);
}
if (issues.length) {
  console.error(issues.join('\n')); process.exitCode = 1;
} else console.log('I18N catalogs and application UI text checked.');
