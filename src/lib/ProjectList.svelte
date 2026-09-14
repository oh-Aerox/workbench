<script>
  import { relTime } from './format.js'

  let { projects, activeProject, loading, onselect } = $props()
</script>

<section class="col">
  <header>
    项目
    {#if projects.length}<span class="count">{projects.length}</span>{/if}
  </header>

  <div class="body">
    {#if loading}
      <div class="hint">扫描中…</div>
    {:else if !projects.length}
      <div class="hint">该 agent 下没有项目记录</div>
    {:else}
      {#each projects as p (p.id)}
        <button class="item" class:active={activeProject === p.id} onclick={() => onselect(p)}>
          <div class="row">
            <span class="name">{p.name}</span>
            {#if p.running}<span class="live" title="最近仍在写入">●</span>{/if}
          </div>
          <div class="path" title={p.path}>{p.path}</div>
          <div class="row sub">
            <span>{p.sessionCount} 场会话</span>
            <span class="when">{relTime(p.lastActive)}</span>
          </div>
        </button>
      {/each}
    {/if}
  </div>
</section>

<style>
  .col {
    display: flex;
    flex-direction: column;
    background: var(--panel);
    border-right: 1px solid var(--line);
    min-height: 0;
  }
  header {
    padding: 12px 14px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: var(--dim);
    border-bottom: 1px solid var(--line);
    display: flex;
    gap: 6px;
  }
  .count { color: var(--dimmer); font-weight: 400; }

  .body { overflow-y: auto; padding: 6px; flex: 1; }

  .hint {
    padding: 24px 14px;
    color: var(--dimmer);
    font-size: 12px;
    text-align: center;
  }

  .item {
    display: block;
    width: 100%;
    text-align: left;
    padding: 9px 10px;
    border-radius: 6px;
    margin-bottom: 2px;
  }
  .item:hover { background: var(--panel-2); }
  .item.active { background: var(--panel-2); box-shadow: inset 2px 0 0 var(--accent); }

  .row { display: flex; align-items: center; gap: 6px; }
  .name { font-weight: 500; }
  .live { color: var(--ok); font-size: 9px; }

  .path {
    font-size: 10px;
    color: var(--dimmer);
    margin: 3px 0 5px;
    direction: rtl;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sub { font-size: 11px; color: var(--dim); justify-content: space-between; }
  .when { color: var(--dimmer); }
</style>
