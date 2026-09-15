<script>
  import Markdown from './Markdown.svelte'
  import RepoTodos from './RepoTodos.svelte'
  import { relTime } from './format.js'

  let { plans, loading, agent, repoReport, repoScanning, repoError, onscan } = $props()

  // 默认展开最新一份，其余折叠
  let open = $state(new Set([0]))

  function toggle(i) {
    const next = new Set(open)
    next.has(i) ? next.delete(i) : next.add(i)
    open = next
  }

  function done(items) {
    return items.filter((t) => t.status === 'completed').length
  }
</script>

{#if loading}
  <div class="hint">提取中…</div>
{:else}
{#if !plans.length}
  <div class="empty">
    <p class="lead">该项目没有 agent 计划记录</p>
    <p>这一栏读的是 agent 自己产出的计划，来源：</p>
    <ul>
      <li><b>Claude Code</b> — 计划模式（<code>ExitPlanMode</code>）产出的实施计划、<code>TodoWrite</code> 任务清单、<code>~/.claude/plans/*.md</code></li>
      <li><b>Codex</b> — <code>update_plan</code> 工具的步骤清单</li>
      <li><b>WorkBuddy</b> — 暂无，它的工具集里还没有计划/待办类工具</li>
    </ul>
    <p class="tip">
      {#if agent?.id === 'workbuddy'}
        WorkBuddy 不记录计划，这一栏对它恒为空。
      {:else}
        该项目还没用过计划模式。用 Claude Code 的计划模式做一次规划，这里就会出现。
      {/if}
      项目源码里的 TODO 标记是另一套来源，见下方。
    </p>
  </div>
{:else}
  <div class="wrap">
    {#each plans as p, i (p.sessionId + p.source + i)}
      {@const isOpen = open.has(i)}
      <article class="plan">
        <button class="head" onclick={() => toggle(i)}>
          <span class="caret" class:open={isOpen}>▸</span>
          <span class="title">{p.title}</span>
          {#if p.items.length}
            <span class="progress">{done(p.items)}/{p.items.length}</span>
          {/if}
          <span class="src">{p.source}</span>
          <span class="when">{relTime(p.at)}</span>
        </button>

        {#if p.items.length}
          <div class="bar">
            <i style="width: {(done(p.items) / p.items.length) * 100}%"></i>
          </div>
        {/if}

        {#if isOpen}
          <div class="body">
            {#if p.items.length && !p.body}
              <!-- TodoWrite / update_plan 是结构化清单，没有正文 -->
              {#each p.items as t}
                <div class="todo {t.status}">
                  <i class="box">{t.status === 'completed' ? '✓' : t.status === 'in_progress' ? '▸' : ''}</i>
                  <span>{t.text}</span>
                </div>
              {/each}
            {:else if p.body}
              <Markdown source={p.body} />
            {/if}
            <p class="from">来自会话「{p.sessionTitle}」</p>
          </div>
        {/if}
      </article>
    {/each}
  </div>
{/if}

<RepoTodos report={repoReport} scanning={repoScanning} error={repoError} {onscan} />
{/if}

<style>
  .hint { padding: 40px 16px; color: var(--dimmer); text-align: center; }

  .empty { padding: 28px 20px; color: var(--dim); font-size: 12px; line-height: 1.7; max-width: 560px; }
  .lead { color: var(--text); font-size: 13px; font-weight: 500; margin: 0 0 10px; }
  .empty p { margin: 0 0 8px; }
  .empty ul { margin: 6px 0 12px; padding-left: 18px; }
  .empty li { margin-bottom: 4px; }
  .empty code {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    background: var(--panel-2);
    border-radius: 3px;
    padding: 1px 4px;
  }
  .tip { color: var(--dimmer); }

  .wrap { padding: 10px; }

  .plan {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 7px;
    margin-bottom: 8px;
    overflow: hidden;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    padding: 10px 12px;
  }
  .head:hover { background: var(--panel-2); }

  .caret {
    color: var(--dimmer);
    font-size: 10px;
    transition: transform 0.12s;
    flex-shrink: 0;
  }
  .caret.open { transform: rotate(90deg); }

  .title {
    font-weight: 500;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .progress {
    font-size: 10px;
    color: var(--ok);
    border: 1px solid currentColor;
    border-radius: 3px;
    padding: 0 4px;
    flex-shrink: 0;
  }
  .src {
    margin-left: auto;
    font-size: 10px;
    color: var(--dimmer);
    background: var(--panel-2);
    border-radius: 3px;
    padding: 1px 5px;
    flex-shrink: 0;
  }
  .when { font-size: 10px; color: var(--dimmer); flex-shrink: 0; }

  .bar { height: 2px; background: var(--panel-2); }
  .bar i { display: block; height: 100%; background: var(--ok); }

  .body { padding: 4px 14px 12px; border-top: 1px solid var(--line); }

  .todo { display: flex; gap: 8px; align-items: flex-start; margin: 6px 0; font-size: 12px; }
  .box {
    width: 13px;
    height: 13px;
    border: 1px solid var(--line);
    border-radius: 3px;
    font-size: 10px;
    line-height: 12px;
    text-align: center;
    flex-shrink: 0;
    margin-top: 2px;
    font-style: normal;
  }
  .todo.completed .box { background: var(--ok); border-color: var(--ok); color: #0b2a16; }
  .todo.completed > span { color: var(--dim); text-decoration: line-through; }
  .todo.in_progress .box { border-color: var(--accent); color: var(--accent); }

  .from { margin: 14px 0 0; font-size: 10px; color: var(--dimmer); }
</style>
