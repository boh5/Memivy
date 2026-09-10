import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = (path) => JSON.parse(readFileSync(new URL(`../${path}`, import.meta.url), 'utf8'));
const fixture = (file) => read(`crates/memivy-core/tests/fixtures/${file}.json`);
const unique = (rows) => assert.equal(new Set(rows.map(r => r.id)).size, rows.length);

test('organization review accounts for every fixed case and fault without relabeling', () => {
  const cases = fixture('phase5_cases');
  const review = fixture('organization_review').cases;
  const faults = fixture('organization_failures');
  unique([...cases, ...faults]);
  assert.equal(cases.length, 40);
  assert.equal(faults.length, 8);
  assert.deepEqual(review.map(r => [r.id, r.expected, r.target]), cases.map(r => [r.id, r.expected, r.target]));
  assert.ok(review.every(r => r.rubric.trim()));
  for (const c of cases.filter(c => c.expected === 'merge')) assert.ok(c.seeds.some(([title]) => title === c.target));
});

test('search and discussion references resolve; a continuing topic retains one seed set', () => {
  for (const name of ['core_search', 'core_discussion']) {
    const data = fixture(name);
    unique(data.seeds);
    const cases = data.queries ?? data.cases;
    unique(cases);
    const ids = new Set(data.seeds.map(s => s.id));
    const topics = new Map();
    for (const c of cases) {
      for (const id of [...(c.expected ?? []), ...(c.seeds ?? []), ...(c.expected_sources ?? []), ...[c.since_at, c.until_at, c.raw_hit].filter(Boolean)]) assert.ok(ids.has(id), `${c.id}: ${id}`);
      if (c.topic) {
        if (topics.has(c.topic)) assert.deepEqual(c.seeds, topics.get(c.topic));
        topics.set(c.topic, c.seeds);
        assert.ok(c.rubric.trim());
        for (const id of c.expected_sources) assert.ok(c.seeds.includes(id));
      }
    }
  }
});

test('visual fixture records current source authority and actual supported sizes', () => {
  const contract = read('tests/assets/visual_contract.json');
  const config = read('src-tauri/tauri.conf.json').app.windows.find(w => w.label === 'main');
  assert.deepEqual(contract.window_sizes, [[config.width, config.height], [config.minWidth, config.minHeight]]);
  unique(contract.cases);
  assert.ok(contract.cases.every(c => c.steps && c.check && c.surface));
  assert.ok(contract.evidence_fields.includes('source_sha256'));
});

test('conclusion contract points to executable regressions, not missing checklist items', () => {
  const contract = read('tests/assets/conclusion_contract.json');
  unique(contract.cases);
  for (const c of contract.cases) {
    const source = readFileSync(new URL(`../${c.file}`, import.meta.url), 'utf8');
    assert.ok(source.includes(`fn ${c.test}(`), c.id);
    assert.ok(c.check.trim());
  }
});
