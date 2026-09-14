<script>
  import Heatmap from './Heatmap.svelte'
  import { relTime } from './format.js'

  let { agent, days, loading } = $props()

  let totalEvents = $derived((days ?? []).reduce((a, d) => a + d.events, 0))
  let activeDays = $derived((days ?? []).length)
  // 连续活跃天数：从今天（或最近活跃日）往回数不断档的天数
  let streak = $derived.by(() => {
    if (!days?.length) return 0
    const set = new Set(days.map((d) => d.day))
    const cur = new Date()
    cur.setHours(0, 0, 0, 0)
    const key = (d) =>
      `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
    // 今天还没活动不算断档，从昨天起算
    if (!set.has(key(cur))) cur.setDate(cur.getDate() - 1)
    let n = 0
    while (set.has(key(cur))) {
      n++
      cur.setDate(cur.getDate() - 1)
    }
    return n
  })
</script>

<div class="wrap">
  {#if loading}
    <p class="hint">首次统计需要全量解析，之后走缓存…</p>
  {:else if !days?.length}
    <p class="hint">该 agent 下还没有活动记录</p>
  {:else}
    <div class="stats">
      <div class="stat"><b>{agent?.projectCount ?? 0}</b><span>项目</span></div>
      <div class="stat"><b>{agent?.sessionCount ?? 0}</b><span>会话</span></div>
      <div class="stat"><b>{activeDays}</b><span>活跃天数</span></div>
      <div class="stat"><b>{totalEvents.toLocaleString()}</b><span>活动事件</span></div>
      <div class="stat"><b>{streak}</b><span>连续活跃</span></div>
      <div class="stat"><b>{relTime(agent?.lastActive)}</b><span>最后活动</span></div>
    </div>

    <section>
      <h3>活动热力图 <span class="n">近 26 周</span></h3>
      <Heatmap {days} />
    </section>

    <p class="tip">从左侧选一个项目，查看它的成果盘点。</p>
  {/if}
</div>

<style>
  .wrap { padding: 14px 16px; }
  .hint { color: var(--dimmer); text-align: center; padding: 40px 0; margin: 0; }

  .stats { display: flex; flex-wrap: wrap; gap: 10px; }
  .stat {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 84px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 8px 11px;
  }
  .stat b { font-size: 15px; font-weight: 600; }
  .stat span { font-size: 10px; color: var(--dim); }

  section { margin-top: 20px; }
  h3 {
    margin: 0 0 12px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .n { color: var(--dimmer); font-weight: 400; letter-spacing: 0; }

  .tip { margin-top: 24px; font-size: 11px; color: var(--dimmer); }
</style>
