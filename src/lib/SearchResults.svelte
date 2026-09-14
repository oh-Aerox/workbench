<script>
  import { relTime } from './format.js'

  let { hits, query, loading, onopen } = $props()

  const FIELD_LABEL = {
    title: '标题',
    prompt: '提问',
    file: '文件',
    project: '项目',
  }

  const AGENT_LABEL = {
    claude: 'Claude Code',
    codex: 'Codex',
    workbuddy: 'WorkBuddy',
  }

  /** 把命中的关键词切出来，供高亮。大小写不敏感，保留原文大小写。 */
  function parts(text, q) {
    if (!q) return [{ t: text, hit: false }]
    const lower = text.toLowerCase()
    const needle = q.toLowerCase()
    const out = []
    let i = 0
    while (i < text.length) {
      const at = lower.indexOf(needle, i)
      if (at < 0) {
        out.push({ t: text.slice(i), hit: false })
        break
      }
      if (at > i) out.push({ t: text.slice(i, at), hit: false })
      out.push({ t: text.slice(at, at + needle.length), hit: true })
      i = at + needle.length
    }
    return out
  }
</script>

{#if loading}
  <div class="hint">搜索中…</div>
{:else if !hits.length}
  <div class="hint">没有匹配「{query}」的会话</div>
{:else}
  <div class="wrap">
    <p class="count">命中 {hits.length} 条</p>
    {#each hits as h (h.agent + h.sessionId + h.field)}
      <button class="hit" onclick={() => onopen(h)}>
        <div class="top">
          <span class="agent {h.agent}">{AGENT_LABEL[h.agent] ?? h.agent}</span>
          <span class="proj">{h.projectName}</span>
          <span class="when">{relTime(h.startedAt)}</span>
        </div>
        <div class="title">
          {#each parts(h.title, query) as p}<span class:hl={p.hit}>{p.t}</span>{/each}
        </div>
        {#if h.field !== 'title'}
          <div class="snippet">
            <span class="field">{FIELD_LABEL[h.field] ?? h.field}</span>
            {#each parts(h.snippet, query) as p}<span class:hl={p.hit}>{p.t}</span>{/each}
          </div>
        {/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .hint { padding: 40px 16px; color: var(--dimmer); text-align: center; }
  .wrap { padding: 10px; }
  .count { margin: 2px 6px 8px; font-size: 11px; color: var(--dimmer); }

  .hit {
    display: block;
    width: 100%;
    text-align: left;
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 7px;
    padding: 10px 12px;
    margin-bottom: 7px;
  }
  .hit:hover { border-color: var(--accent); }

  .top { display: flex; align-items: center; gap: 7px; font-size: 10px; }
  .agent { font-weight: 600; }
  .agent.claude { color: var(--claude); }
  .agent.codex { color: var(--codex); }
  .agent.workbuddy { color: var(--workbuddy); }
  .proj { color: var(--dim); }
  .when { margin-left: auto; color: var(--dimmer); }

  .title {
    margin-top: 5px;
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .snippet {
    margin-top: 5px;
    font-size: 11px;
    color: var(--dim);
    line-height: 1.5;
    word-break: break-all;
  }
  .field {
    display: inline-block;
    font-size: 9px;
    color: var(--dimmer);
    border: 1px solid var(--line);
    border-radius: 3px;
    padding: 0 4px;
    margin-right: 5px;
  }

  .hl { background: color-mix(in srgb, var(--accent) 35%, transparent); border-radius: 2px; }
</style>
