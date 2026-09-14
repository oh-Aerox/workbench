<script>
  import { relTime, duration, cost, bytes, shortModel } from './format.js'

  let { sessions, loading, hasProject } = $props()
</script>

{#if loading}
  <div class="hint">解析会话中…大会话首次打开需要几秒</div>
{:else if !hasProject}
  <div class="hint">从左侧选一个项目</div>
{:else if !sessions.length}
  <div class="hint">该项目下没有会话</div>
{:else}
  <div class="wrap">
    {#each sessions as s (s.id)}
      <article class="card">
        <div class="top">
          <span class="title">{s.title}</span>
          {#if s.running}<span class="live">进行中</span>{/if}
          <span class="when">{relTime(s.startedAt)}</span>
        </div>

        <div class="stats">
          <span><b>{duration(s.wallMs)}</b> 时长</span>
          <span><b>{s.userTurns}</b> 轮对话</span>
          <span><b>{s.toolCalls}</b> 次工具调用</span>
          <span><b>{s.filesTouched}</b> 个文件</span>
          {#if s.linesAdded || s.linesRemoved}
            <span class="add">+{s.linesAdded}</span><span class="del">−{s.linesRemoved}</span>
          {/if}
          {#if s.costUsd != null}<span>{cost(s.costUsd)}</span>{/if}
        </div>

        <div class="tags">
          {#each s.models as m}<span class="chip">{shortModel(m)}</span>{/each}
          {#if s.gitBranch && s.gitBranch !== 'HEAD'}
            <span class="chip branch">{s.gitBranch}</span>
          {/if}
          <span class="size">{bytes(s.bytes)}</span>
        </div>
      </article>
    {/each}
  </div>
{/if}

<style>
  .hint { padding: 40px 16px; color: var(--dimmer); text-align: center; }
  .wrap { padding: 10px; }

  .card {
    background: var(--panel);
    border: 1px solid var(--line);
    border-radius: 7px;
    padding: 11px 13px;
    margin-bottom: 8px;
  }

  .top { display: flex; align-items: baseline; gap: 8px; }
  .title {
    font-weight: 500;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    user-select: text;
  }
  .live {
    font-size: 10px;
    color: var(--ok);
    border: 1px solid currentColor;
    border-radius: 3px;
    padding: 0 4px;
    flex-shrink: 0;
  }
  .when { margin-left: auto; font-size: 11px; color: var(--dimmer); flex-shrink: 0; }

  .stats {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin-top: 8px;
    font-size: 11px;
    color: var(--dim);
  }
  .stats b { color: var(--text); font-weight: 600; }
  .add { color: var(--ok); }
  .del { color: #f87171; }

  .tags { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; margin-top: 9px; }
  .chip {
    font-size: 10px;
    color: var(--dim);
    background: var(--panel-2);
    border-radius: 3px;
    padding: 2px 6px;
  }
  .branch { color: var(--accent); }
  .size { margin-left: auto; font-size: 10px; color: var(--dimmer); }
</style>
