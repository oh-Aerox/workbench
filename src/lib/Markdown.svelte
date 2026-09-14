<script>
  /**
   * 极简 markdown 渲染：标题、复选框、列表、代码块、引用、段落，
   * 行内支持 `代码` 和 **粗体**。
   *
   * 刻意不引 markdown 库（省体积），也刻意不用 {@html}——计划正文虽然出自
   * 用户自己的 agent，但仍是外部数据，走 Svelte 模板渲染就没有注入面。
   */
  let { source } = $props()

  const H = /^(#{1,6})\s+(.*)$/
  const TODO = /^\s*[-*]\s+\[([ xX])\]\s*(.*)$/
  const LI = /^(\s*)[-*]\s+(.*)$/
  const OL = /^(\s*)(\d+)\.\s+(.*)$/
  const QUOTE = /^>\s?(.*)$/

  let blocks = $derived.by(() => {
    const lines = (source ?? '').split('\n')
    const out = []
    let para = []

    const flush = () => {
      if (para.length) {
        out.push({ type: 'p', text: para.join(' ') })
        para = []
      }
    }

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i]

      if (/^\s*```/.test(line)) {
        flush()
        const lang = line.replace(/^\s*```/, '').trim()
        const body = []
        i++
        while (i < lines.length && !/^\s*```/.test(lines[i])) body.push(lines[i++])
        out.push({ type: 'code', lang, text: body.join('\n') })
        continue
      }

      if (!line.trim()) {
        flush()
        continue
      }

      let m
      if ((m = line.match(H))) {
        flush()
        out.push({ type: 'h', level: m[1].length, text: m[2] })
      } else if ((m = line.match(TODO))) {
        flush()
        out.push({ type: 'todo', checked: m[1].toLowerCase() === 'x', text: m[2] })
      } else if ((m = line.match(LI))) {
        flush()
        out.push({ type: 'li', depth: Math.floor(m[1].length / 2), text: m[2] })
      } else if ((m = line.match(OL))) {
        flush()
        out.push({ type: 'li', depth: Math.floor(m[1].length / 2), num: m[2], text: m[3] })
      } else if ((m = line.match(QUOTE))) {
        flush()
        out.push({ type: 'quote', text: m[1] })
      } else if (/^\s*\|/.test(line) || /^\s*[-=]{3,}\s*$/.test(line)) {
        // 表格和分隔线不做结构化，按原样单行展示，避免误解析
        flush()
        out.push({ type: 'raw', text: line })
      } else {
        para.push(line.trim())
      }
    }
    flush()
    return out
  })

  /** 行内切分：`代码` 与 **粗体** */
  function inline(text) {
    const out = []
    const re = /(`[^`]+`|\*\*[^*]+\*\*)/g
    let last = 0
    let m
    while ((m = re.exec(text))) {
      if (m.index > last) out.push({ t: text.slice(last, m.index) })
      const tok = m[0]
      if (tok.startsWith('`')) out.push({ t: tok.slice(1, -1), code: true })
      else out.push({ t: tok.slice(2, -2), bold: true })
      last = m.index + tok.length
    }
    if (last < text.length) out.push({ t: text.slice(last) })
    return out
  }
</script>

<div class="md">
  {#each blocks as b}
    {#if b.type === 'h'}
      <div class="h h{b.level}">
        {#each inline(b.text) as p}<span class:code={p.code} class:bold={p.bold}>{p.t}</span>{/each}
      </div>
    {:else if b.type === 'todo'}
      <div class="todo" class:done={b.checked}>
        <i class="box">{b.checked ? '✓' : ''}</i>
        <span>{#each inline(b.text) as p}<span class:code={p.code} class:bold={p.bold}>{p.t}</span>{/each}</span>
      </div>
    {:else if b.type === 'li'}
      <div class="li" style="padding-left: {8 + b.depth * 14}px">
        <i class="dot">{b.num ? b.num + '.' : '·'}</i>
        <span>{#each inline(b.text) as p}<span class:code={p.code} class:bold={p.bold}>{p.t}</span>{/each}</span>
      </div>
    {:else if b.type === 'code'}
      <pre class="code-block">{b.text}</pre>
    {:else if b.type === 'quote'}
      <div class="quote">
        {#each inline(b.text) as p}<span class:code={p.code} class:bold={p.bold}>{p.t}</span>{/each}
      </div>
    {:else if b.type === 'raw'}
      <div class="raw">{b.text}</div>
    {:else}
      <p>{#each inline(b.text) as p}<span class:code={p.code} class:bold={p.bold}>{p.t}</span>{/each}</p>
    {/if}
  {/each}
</div>

<style>
  .md { font-size: 12px; line-height: 1.65; user-select: text; }

  .h { font-weight: 600; margin: 14px 0 6px; }
  .h1 { font-size: 15px; }
  .h2 { font-size: 13.5px; color: var(--accent); }
  .h3 { font-size: 12.5px; }
  .h4, .h5, .h6 { font-size: 12px; color: var(--dim); }

  p { margin: 6px 0; color: var(--text); }

  .li { display: flex; gap: 7px; margin: 3px 0; }
  .dot { color: var(--dimmer); flex-shrink: 0; font-style: normal; min-width: 12px; }

  .todo { display: flex; gap: 7px; margin: 4px 0; align-items: flex-start; }
  .box {
    width: 13px;
    height: 13px;
    border: 1px solid var(--line);
    border-radius: 3px;
    font-size: 10px;
    line-height: 12px;
    text-align: center;
    flex-shrink: 0;
    margin-top: 3px;
    font-style: normal;
  }
  .todo.done .box { background: var(--ok); border-color: var(--ok); color: #0b2a16; }
  .todo.done > span { color: var(--dim); text-decoration: line-through; }

  .code-block {
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: 5px;
    padding: 9px 11px;
    margin: 8px 0;
    overflow-x: auto;
    font-size: 11px;
    line-height: 1.5;
    font-family: ui-monospace, Consolas, monospace;
  }

  .quote {
    border-left: 2px solid var(--line);
    padding-left: 10px;
    margin: 6px 0;
    color: var(--dim);
  }

  .raw {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    color: var(--dim);
    white-space: pre;
    overflow-x: auto;
  }

  .code {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    background: var(--panel-2);
    border-radius: 3px;
    padding: 1px 4px;
  }
  .bold { font-weight: 600; }
</style>
