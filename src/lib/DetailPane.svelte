<script>
  import OutcomeView from './OutcomeView.svelte'
  import SessionList from './SessionList.svelte'
  import AgentOverview from './AgentOverview.svelte'
  import SearchResults from './SearchResults.svelte'
  import PlansView from './PlansView.svelte'

  let {
    agent,
    outcome,
    sessions,
    plans,
    days,
    loading,
    loadingActivity,
    hits,
    query,
    searching,
    hasProject,
    projectName,
    onopen,
  } = $props()

  // 默认落在「成果」：先看项目被改成了什么样，再按需回看过程
  let tab = $state('outcome')

  // 三态：搜索中 > 选了项目看详情 > 没选项目看 agent 概览
  let mode = $derived(query.trim() ? 'search' : hasProject ? 'project' : 'overview')
</script>

<section class="col">
  <header>
    {#if mode === 'project'}
      <div class="tabs">
        <button class:on={tab === 'outcome'} onclick={() => (tab = 'outcome')}>成果</button>
        <button class:on={tab === 'plans'} onclick={() => (tab = 'plans')}>
          计划
          {#if plans.length}<span class="n">{plans.length}</span>{/if}
        </button>
        <button class:on={tab === 'sessions'} onclick={() => (tab = 'sessions')}>
          会话
          {#if sessions.length}<span class="n">{sessions.length}</span>{/if}
        </button>
      </div>
      <span class="sub">{projectName}</span>
    {:else if mode === 'search'}
      <span class="label">搜索「{query.trim()}」</span>
    {:else}
      <span class="label">{agent?.displayName ?? ''} 概览</span>
      <span class="sub">{agent?.root ?? ''}</span>
    {/if}
  </header>

  <div class="body">
    {#if mode === 'search'}
      <SearchResults {hits} query={query.trim()} loading={searching} {onopen} />
    {:else if mode === 'overview'}
      <AgentOverview {agent} {days} loading={loadingActivity} />
    {:else if tab === 'outcome'}
      <OutcomeView {outcome} {loading} />
    {:else if tab === 'plans'}
      <PlansView {plans} {loading} {agent} />
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
    min-height: 42px;
  }

  .label { font-size: 12px; font-weight: 500; }

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

  .sub {
    margin-left: auto;
    font-size: 11px;
    color: var(--dimmer);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
  }

  .body { overflow-y: auto; flex: 1; }
</style>
