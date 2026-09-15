<script>
  let { report, scanning, error, onscan } = $props()

  let openFiles = $state(new Set())

  function toggle(f) {
    const next = new Set(openFiles)
    next.has(f) ? next.delete(f) : next.add(f)
    openFiles = next
  }

  // 按文件分组，命中多的排前面
  let grouped = $derived.by(() => {
    if (!report?.todos?.length) return []
    const map = new Map()
    for (const t of report.todos) {
      if (!map.has(t.file)) map.set(t.file, [])
      map.get(t.file).push(t)
    }
    return [...map.entries()]
      .map(([file, items]) => ({ file, items }))
      .sort((a, b) => b.items.length - a.items.length)
  })

  let byMarker = $derived.by(() => {
    const c = {}
    for (const t of report?.todos ?? []) c[t.marker] = (c[t.marker] ?? 0) + 1
    return Object.entries(c).sort((a, b) => b[1] - a[1])
  })
</script>

<section class="repo">
  <h3>
    源码待办
    {#if report?.todos?.length}<span class="n">{report.todos.length}</span>{/if}
    <button class="scan" onclick={onscan} disabled={scanning}>
      {scanning ? '扫描中…' : report ? '重新扫描' : '扫描源码'}
    </button>
  </h3>

  {#if error}
    <p class="err">{error}</p>
  {:else if scanning}
    <p class="note">正在遍历项目源码…大项目首次扫描可能要几十秒，之后走缓存会快很多。</p>
  {:else if !report}
    <p class="note">
      扫描项目工作目录里的 <code>TODO</code> / <code>FIXME</code> / <code>XXX</code> /
      <code>HACK</code> 标记和 <code>TODO.md</code> 类文档。
      <br />
      这会读取项目源码（仍然只读），与上方「agent 计划」的数据来源不同，所以做成手动触发。
    </p>
  {:else if !report.exists}
    <p class="note">项目目录已不存在：<code>{report.root}</code></p>
  {:else}
    <div class="meta">
      <span>扫描 <b>{report.filesScanned.toLocaleString()}</b> 个文件</span>
      <span>耗时 <b>{(report.elapsedMs / 1000).toFixed(1)}s</b></span>
      {#each byMarker as [m, n]}<span class="mk">{m} <b>{n}</b></span>{/each}
    </div>

    {#if report.truncated}
      <p class="warn">结果已截断：触到遍历或命中上限，下面不是全部。</p>
    {/if}

    {#if report.docs.length}
      <div class="docs">
        {#each report.docs as d}
          <div class="doc">
            <span class="dfile">{d.file}</span>
            <span class="dprog">{d.done}/{d.total}</span>
            <div class="dbar"><i style="width: {d.total ? (d.done / d.total) * 100 : 0}%"></i></div>
          </div>
        {/each}
      </div>
    {/if}

    {#if !report.todos.length}
      <p class="note">没有找到 TODO 标记。</p>
    {:else}
      <ul class="files">
        {#each grouped.slice(0, 60) as g (g.file)}
          {@const isOpen = openFiles.has(g.file)}
          <li>
            <button class="frow" onclick={() => toggle(g.file)}>
              <span class="caret" class:open={isOpen}>▸</span>
              <span class="fname" title={g.file}>{g.file}</span>
              <span class="fcount">{g.items.length}</span>
            </button>
            {#if isOpen}
              <div class="hits">
                {#each g.items as t}
                  <div class="hit">
                    <span class="ln">{t.line}</span>
                    <span class="mark {t.marker}">{t.marker}</span>
                    <span class="txt">{t.text}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      {#if grouped.length > 60}
        <p class="note">另有 {grouped.length - 60} 个文件未显示</p>
      {/if}
    {/if}
  {/if}
</section>

<style>
  .repo { padding: 4px 16px 16px; border-top: 1px solid var(--line); margin-top: 6px; }

  h3 {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 14px 0 10px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  .n { color: var(--dimmer); font-weight: 400; }

  .scan {
    margin-left: auto;
    font-size: 11px;
    font-weight: 400;
    letter-spacing: 0;
    color: var(--accent);
    border: 1px solid var(--line);
    border-radius: 5px;
    padding: 3px 10px;
  }
  .scan:hover:not(:disabled) { border-color: var(--accent); background: var(--panel); }
  .scan:disabled { color: var(--dimmer); cursor: default; }

  .note { font-size: 11px; color: var(--dimmer); line-height: 1.7; margin: 0 0 10px; max-width: 560px; }
  .warn { font-size: 11px; color: #fbbf24; margin: 0 0 10px; }
  .err { font-size: 11px; color: #ffb4b4; margin: 0 0 10px; user-select: text; }

  code {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 10.5px;
    background: var(--panel-2);
    border-radius: 3px;
    padding: 1px 4px;
  }

  .meta { display: flex; flex-wrap: wrap; gap: 12px; font-size: 11px; color: var(--dim); margin-bottom: 10px; }
  .meta b { color: var(--text); }
  .mk { color: var(--dimmer); }

  .docs { margin-bottom: 12px; }
  .doc { display: flex; align-items: center; gap: 8px; padding: 5px 8px; font-size: 12px; }
  .dfile { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dprog { font-size: 10px; color: var(--ok); }
  .dbar { width: 70px; height: 3px; background: var(--panel-2); border-radius: 2px; flex-shrink: 0; }
  .dbar i { display: block; height: 100%; background: var(--ok); border-radius: 2px; }

  ul { list-style: none; margin: 0; padding: 0; }

  .frow {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    text-align: left;
    padding: 5px 8px;
    border-radius: 4px;
    font-size: 12px;
  }
  .frow:hover { background: var(--panel); }
  .caret { color: var(--dimmer); font-size: 9px; flex-shrink: 0; transition: transform 0.12s; }
  .caret.open { transform: rotate(90deg); }
  .fname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .fcount {
    font-size: 10px;
    color: var(--dimmer);
    background: var(--panel-2);
    border-radius: 3px;
    padding: 1px 5px;
    flex-shrink: 0;
  }

  .hits { padding: 2px 0 6px 22px; }
  .hit { display: flex; gap: 8px; padding: 3px 0; font-size: 11px; align-items: baseline; }
  .ln {
    color: var(--dimmer);
    font-family: ui-monospace, Consolas, monospace;
    min-width: 38px;
    text-align: right;
    flex-shrink: 0;
  }
  .mark {
    font-size: 9px;
    font-weight: 600;
    border-radius: 3px;
    padding: 0 4px;
    flex-shrink: 0;
    background: var(--panel-2);
  }
  .mark.TODO { color: var(--accent); }
  .mark.FIXME { color: #f87171; }
  .mark.XXX { color: #fbbf24; }
  .mark.HACK { color: var(--workbuddy); }
  .txt { color: var(--dim); word-break: break-word; user-select: text; }
</style>
