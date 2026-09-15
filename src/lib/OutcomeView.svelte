<script>
  import { relTime, duration, cost } from './format.js'

  let { outcome, loading } = $props()

  // 条形图按最大改动次数归一化
  let maxTouches = $derived(
    outcome?.files?.length ? Math.max(...outcome.files.map((f) => f.touches)) : 1
  )
  // 只有增删行数非零的会话才值得进贡献排行
  let contributors = $derived(
    (outcome?.topSessions ?? []).filter((s) => s.linesAdded + s.linesRemoved > 0).slice(0, 8)
  )
  let maxLines = $derived(
    contributors.length ? contributors[0].linesAdded + contributors[0].linesRemoved : 1
  )
</script>

{#if loading}
  <div class="hint">聚合中…</div>
{:else if !outcome}
  <div class="hint">从左侧选一个项目</div>
{:else}
  <div class="stats">
    <div class="stat"><b>{outcome.sessionCount}</b><span>会话</span></div>
    <div class="stat"><b>{duration(outcome.totalWallMs)}</b><span>累计时长</span></div>
    <div class="stat"><b>{outcome.totalToolCalls}</b><span>工具调用</span></div>
    <div class="stat"><b>{outcome.files.length}</b><span>触达文件</span></div>
    <div class="stat">
      <b><span class="add">+{outcome.totalLinesAdded}</span> <span class="del">−{outcome.totalLinesRemoved}</span></b>
      <span>增删行数</span>
    </div>
    <div class="stat"><b>{cost(outcome.totalCostUsd)}</b><span>花费</span></div>
  </div>

  <section>
    <h3>文件改动排行 <span class="n">{outcome.files.length}</span></h3>
    {#if !outcome.files.length}
      <p class="empty">
        该 agent 不记录文件改动历史，这里没有数据可显示。
      </p>
    {:else}
      <ul class="files">
        {#each outcome.files.slice(0, 60) as f (f.path)}
          <li>
            <div class="bar" style="width: {(f.touches / maxTouches) * 100}%"></div>
            <span class="rel" class:outside={f.outside} title={f.path}>{f.rel}</span>
            {#if f.outside}<span class="flag" title="不在项目目录内">项目外</span>{/if}
            <span class="num">{f.touches} 次</span>
            {#if f.sessions > 1}<span class="num dim">{f.sessions} 场</span>{/if}
            <span class="when">{relTime(f.lastTouched)}</span>
          </li>
        {/each}
      </ul>
      {#if outcome.files.length > 60}
        <p class="empty">另有 {outcome.files.length - 60} 个文件未显示</p>
      {/if}
    {/if}
  </section>

  <section>
    <h3>会话贡献排行</h3>
    {#if !contributors.length}
      <p class="empty">该 agent 不记录增删行数，无法排序会话贡献。</p>
    {:else}
      <ul class="sessions">
        {#each contributors as s (s.id)}
          <li>
            <div class="srow">
              <span class="title" title={s.title}>{s.title}</span>
              <span class="when">{relTime(s.startedAt)}</span>
            </div>
            <div class="srow">
              <div class="sbar" style="width: {((s.linesAdded + s.linesRemoved) / maxLines) * 100}%">
                <i class="ia" style="flex: {s.linesAdded}"></i>
                <i class="id" style="flex: {s.linesRemoved}"></i>
              </div>
              <span class="num add">+{s.linesAdded}</span>
              <span class="num del">−{s.linesRemoved}</span>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .hint { padding: 40px 16px; color: var(--dimmer); text-align: center; }

  .stats {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--line);
  }
  .stat {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 88px;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 8px 11px;
  }
  .stat b { font-size: 15px; font-weight: 600; }
  .stat span { font-size: 10px; color: var(--dim); }

  section { padding: 14px 16px 4px; }
  h3 {
    margin: 0 0 10px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .n { color: var(--dimmer); font-weight: 400; }
  .empty { color: var(--dimmer); font-size: 11px; margin: 0 0 10px; }

  ul { list-style: none; margin: 0; padding: 0; }

  .files li {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    border-radius: 4px;
    font-size: 12px;
  }
  .files li:hover { background: var(--panel); }
  .bar {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    background: var(--accent);
    opacity: 0.12;
    border-radius: 4px;
    pointer-events: none;
  }
  .rel {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
  }
  .rel.outside { color: var(--dim); }
  .flag {
    font-size: 9px;
    color: var(--dimmer);
    border: 1px solid var(--line);
    border-radius: 3px;
    padding: 0 3px;
    flex-shrink: 0;
  }
  .num { font-size: 11px; flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .num.dim { color: var(--dimmer); }
  .when { font-size: 10px; color: var(--dimmer); width: 62px; text-align: right; flex-shrink: 0; }

  .add { color: var(--ok); }
  .del { color: #f87171; }

  .sessions li { padding: 7px 8px; border-radius: 4px; }
  .sessions li:hover { background: var(--panel); }
  .srow { display: flex; align-items: center; gap: 8px; }
  .srow + .srow { margin-top: 5px; }
  .title {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sbar { display: flex; height: 6px; border-radius: 3px; overflow: hidden; min-width: 2px; }
  .ia { background: var(--ok); }
  .id { background: #f87171; }
</style>
