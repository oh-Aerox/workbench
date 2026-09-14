<script>
  import OutcomeView from './OutcomeView.svelte'
  import SessionList from './SessionList.svelte'

  let { outcome, sessions, loading, hasProject, projectName } = $props()

  // 默认落在「成果」：先看项目被改成了什么样，再按需回看过程
  let tab = $state('outcome')
</script>

<section class="col">
  <header>
    <div class="tabs">
      <button class:on={tab === 'outcome'} onclick={() => (tab = 'outcome')}>成果</button>
      <button class:on={tab === 'sessions'} onclick={() => (tab = 'sessions')}>
        会话
        {#if sessions.length}<span class="n">{sessions.length}</span>{/if}
      </button>
    </div>
    {#if projectName}<span class="proj">{projectName}</span>{/if}
  </header>

  <div class="body">
    {#if tab === 'outcome'}
      <OutcomeView {outcome} {loading} />
    {:else}
      <SessionList {sessions} {loading} {hasProject} />
    {/if}
  </div>
</section>

<style>
  .col { display: flex; flex-direction: column; min-height: 0; min-width: 0; }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--line);
  }

  .tabs { display: flex; gap: 2px; }
  .tabs button {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 11px;
    border-radius: 5px;
    font-size: 12px;
    color: var(--dim);
  }
  .tabs button:hover { background: var(--panel); }
  .tabs button.on { background: var(--panel-2); color: var(--text); font-weight: 500; }
  .n { font-size: 10px; color: var(--dimmer); }

  .proj {
    margin-left: auto;
    font-size: 11px;
    color: var(--dimmer);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .body { overflow-y: auto; flex: 1; }
</style>
