<script>
  let { days, weeks = 26 } = $props()

  const WEEKDAYS = ['一', '', '三', '', '五', '', '日']
  const MONTHS = ['1月','2月','3月','4月','5月','6月','7月','8月','9月','10月','11月','12月']

  function ymd(d) {
    // 按本地日期拼串，不能用 toISOString —— 那是 UTC，晚上的活动会错到前一天
    const m = String(d.getMonth() + 1).padStart(2, '0')
    const day = String(d.getDate()).padStart(2, '0')
    return `${d.getFullYear()}-${m}-${day}`
  }

  let lookup = $derived(new Map((days ?? []).map((d) => [d.day, d])))

  // 网格按周分列，每列周一到周日。末列是本周，回推 weeks 列。
  let grid = $derived.by(() => {
    const today = new Date()
    today.setHours(0, 0, 0, 0)
    // getDay(): 0=周日。换算成周一为起点的偏移
    const offsetToMonday = (today.getDay() + 6) % 7
    const lastMonday = new Date(today)
    lastMonday.setDate(today.getDate() - offsetToMonday)

    const cols = []
    for (let w = weeks - 1; w >= 0; w--) {
      const col = []
      for (let d = 0; d < 7; d++) {
        const date = new Date(lastMonday)
        date.setDate(lastMonday.getDate() - w * 7 + d)
        col.push(date > today ? null : { key: ymd(date), date })
      }
      cols.push(col)
    }
    return cols
  })

  let max = $derived(Math.max(1, ...(days ?? []).map((d) => d.events)))

  /** 用 sqrt 压缩量级：单场大会话动辄上千次活动，线性映射会把其余的天全压成同一色 */
  function level(events) {
    if (!events) return 0
    const r = Math.sqrt(events) / Math.sqrt(max)
    return Math.min(4, Math.ceil(r * 4))
  }

  // 每列上方的月份标签，只在当月第一列显示
  let monthLabels = $derived(
    grid.map((col, i) => {
      const first = col.find((c) => c)
      if (!first) return ''
      const prev = i > 0 ? grid[i - 1].find((c) => c) : null
      if (!prev || prev.date.getMonth() !== first.date.getMonth()) {
        return MONTHS[first.date.getMonth()]
      }
      return ''
    })
  )
</script>

<div class="heat">
  <div class="rows">
    {#each WEEKDAYS as w}<span>{w}</span>{/each}
  </div>

  <div class="scroll">
    <div class="months">
      {#each monthLabels as m}<span>{m}</span>{/each}
    </div>
    <div class="cols">
      {#each grid as col}
        <div class="col">
          {#each col as cell}
            {#if cell}
              {@const hit = lookup.get(cell.key)}
              <i
                class="cell l{level(hit?.events ?? 0)}"
                title="{cell.key} · {hit?.events ?? 0} 次活动{hit ? ` · ${hit.sessions} 场会话` : ''}"
              ></i>
            {:else}
              <i class="cell blank"></i>
            {/if}
          {/each}
        </div>
      {/each}
    </div>
  </div>

  <div class="legend">
    <span>少</span>
    {#each [0, 1, 2, 3, 4] as l}<i class="cell l{l}"></i>{/each}
    <span>多</span>
  </div>
</div>

<style>
  .heat { display: flex; align-items: flex-start; gap: 6px; font-size: 10px; }

  .rows {
    display: grid;
    grid-template-rows: repeat(7, 13px);
    gap: 2px;
    padding-top: 15px;
    color: var(--dimmer);
  }
  .rows span { line-height: 13px; }

  .scroll { overflow-x: auto; padding-bottom: 2px; }

  .months {
    display: flex;
    gap: 2px;
    height: 13px;
    color: var(--dimmer);
  }
  .months span { width: 13px; flex-shrink: 0; white-space: nowrap; }

  .cols { display: flex; gap: 2px; }
  .col { display: grid; grid-template-rows: repeat(7, 13px); gap: 2px; }

  .cell {
    width: 13px;
    height: 13px;
    border-radius: 2px;
    display: block;
  }
  .blank { background: none; }
  .l0 { background: var(--panel-2); }
  .l1 { background: color-mix(in srgb, var(--accent) 25%, var(--panel-2)); }
  .l2 { background: color-mix(in srgb, var(--accent) 50%, var(--panel-2)); }
  .l3 { background: color-mix(in srgb, var(--accent) 75%, var(--panel-2)); }
  .l4 { background: var(--accent); }

  .legend {
    display: flex;
    align-items: center;
    gap: 3px;
    margin-left: auto;
    padding-top: 15px;
    color: var(--dimmer);
    flex-shrink: 0;
  }
</style>
