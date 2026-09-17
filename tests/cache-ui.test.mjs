import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import assert from 'node:assert/strict'

const app = readFileSync(new URL('../src/App.svelte', import.meta.url), 'utf8')
function body(name) {
  const start = app.indexOf(`async function ${name}(`)
  assert.ok(start >= 0)
  const brace = app.indexOf('{', start)
  let depth = 1, end = brace + 1
  for (; depth; end++) {
    if (app[end] === '{') depth++
    if (app[end] === '}') depth--
  }
  return app.slice(brace + 1, end - 1)
}
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor

test('清空缓存不触发重新解析', async () => {
  const calls = []
  const run = new AsyncFunction('clearCache', 'refreshCacheSize', 'selectAgent',
    `let clearing = false, error = null, currentAgent = {}, repoCache = new Map();
     ${body('onClearCache')}`)
  await run(async () => calls.push('clear'), async () => calls.push('size'),
    async () => calls.push('reparse'))
  assert.deepEqual(calls, ['clear', 'size'])
})

test('缓存大小响应乱序时保留最新结果，读取失败不显示空', async () => {
  const run = new AsyncFunction('assert', `
    let cacheBytes = 100, cacheSizeRequest = 0;
    const pending = [];
    const cacheSize = () => new Promise((resolve, reject) => pending.push({resolve, reject}));
    async function refreshCacheSize() { ${body('refreshCacheSize')} }
    const first = refreshCacheSize(), second = refreshCacheSize();
    pending[1].resolve(200); await second;
    pending[0].resolve(0); await first;
    assert.equal(cacheBytes, 200);
    const failed = refreshCacheSize();
    pending[2].reject(new Error('unavailable')); await failed;
    assert.equal(cacheBytes, null);
  `)
  await run(assert)
})

test('每次数据查询后通知大小变化，大小查询不循环通知', async () => {
  const source = readFileSync(new URL('../src/lib/api.js', import.meta.url), 'utf8')
  const load = new Function('tauriInvoke', source
    .replace(/^import .*$/m, '')
    .replaceAll('export ', '') + '\nreturn {listAgents, cacheSize, clearCache, onCacheChanged};')
  const api = load(async () => 123)
  let changes = 0
  const unsubscribe = api.onCacheChanged(() => changes++)
  await api.listAgents()
  assert.equal(changes, 1)
  await api.cacheSize()
  assert.equal(changes, 1)
  await api.clearCache()
  assert.equal(changes, 2)
  unsubscribe()
  await api.listAgents()
  assert.equal(changes, 2)
})
