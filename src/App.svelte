<script>
  import { listen } from '@tauri-apps/api/event'
  import {
    listAgents,
    listProjects,
    listSessions,
    projectOutcome,
    projectPlans,
    projectTodos,
    agentActivity,
    search as searchApi,
  } from './lib/api.js'
  import { relTime } from './lib/format.js'
  import ProjectList from './lib/ProjectList.svelte'
  import DetailPane from './lib/DetailPane.svelte'

  let agents = $state([])
  let activeAgent = $state(null)
  let projects = $state([])
  let activeProject = $state(null)
  let activeProjectName = $state('')
  let sessions = $state([])
  let outcome = $state(null)
  let plans = $state([])
  // 源码 TODO 是唯一读 agent 目录之外文件的功能，故手动触发、按项目缓存结果
  let repoReport = $state(null)
  let repoScanning = $state(false)
  let repoError = $state(null)
  const repoCache = new Map()
  let days = $state([])
  let loadingProjects = $state(false)
  let loadingSessions = $state(false)
  let loadingActivity = $state(false)
  let error = $state(null)

  let query = $state('')
  let hits = $state([])
  let searching = $state(false)
  let searchTimer = null

  let currentAgent = $derived(agents.find((a) => a.id === activeAgent) ?? null)

  async function selectAgent(agent, keepProject = false) {
    if (!agent.installed) return
    activeAgent = agent.id
    if (!keepProject) {
      activeProject = null
      activeProjectName = ''
      sessions = []
      outcome = null
      plans = []
      repoReport = null
      repoError = null
    }
    projects = []
    days = []
    loadingProjects = true
    loadingActivity = true
    try {
      projects = await listProjects(agent.id)
    } catch (e) {
      error = String(e)
    } finally {
      loadingProjects = false
    }
    // 热力图要全量解析，单独等，不挡住项目列表
    try {
      days = await agentActivity(agent.id)
    } catch (e) {
      error = String(e)
    } finally {
      loadingActivity = false
    }
  }

  async function selectProject(project) {
    activeProject = project.id
    activeProjectName = project.path
    sessions = []
    outcome = null
    plans = []
    // 切项目时把上一个项目的扫描结果换掉，命中过就直接复用
    repoReport = repoCache.get(project.path) ?? null
    repoError = null
    loadingSessions = true
    try {
      // 三个视图解析的是同一批 JSONL（缓存共用），一次并发取完，切 tab 不用再等
      const [o, s, pl] = await Promise.all([
        projectOutcome(activeAgent, project.id),
        listSessions(activeAgent, project.id),
        projectPlans(activeAgent, project.id),
      ])
      outcome = o
      sessions = s
      plans = pl
    } catch (e) {
      error = String(e)
    } finally {
      loadingSessions = false
    }
  }

  /** 手动触发源码扫描。大项目首次可能几十秒，所以不在切项目时自动跑。 */
  async function scanRepo() {
    const path = activeProjectName
    if (!path || repoScanning) return
    repoScanning = true
    repoError = null
    try {
      const r = await projectTodos(path)
      repoCache.set(path, r)
      // 扫描期间用户可能已经切走了，别把结果盖到别的项目上
      if (activeProjectName === path) repoReport = r
    } catch (e) {
      if (activeProjectName === path) repoError = String(e)
    } finally {
      repoScanning = false
    }
  }

  /** 搜索结果点进去：跳到对应 agent 的对应项目 */
  async function openHit(hit) {
    query = ''
    hits = []
    const agent = agents.find((a) => a.id === hit.agent)
    if (agent && agent.id !== activeAgent) {
      await selectAgent(agent, true)
    }
    const project =
      projects.find((p) => p.id === hit.projectId) ??
      (await listProjects(hit.agent)).find((p) => p.id === hit.projectId)
    if (project) await selectProject(project)
  }

  // 输入去抖，避免每敲一个字都触发一次全量扫描
  function onQueryInput() {
    clearTimeout(searchTimer)
    const q = query.trim()
    if (!q) {
      hits = []
      searching = false
      return
    }
    searching = true
    searchTimer = setTimeout(async () => {
      try {
        hits = await searchApi(q)
      } catch (e) {
        error = String(e)
      } finally {
        searching = false
      }
    }, 280)
  }

  async function refreshAgents() {
    try {
      agents = await listAgents()
    } catch (e) {
      error = String(e)
    }
  }

  $effect(() => {
    listAgents()
      .then((list) => {
        agents = list
        const first = list.find((a) => a.installed)
        if (first) selectAgent(first)
      })
      .catch((e) => (error = String(e)))

    // agent 在后台写会话文件时自动刷新当前视图
    const un = listen('agent-data-changed', async (ev) => {
      await refreshAgents()
      if (ev.payload !== activeAgent) return
      projects = await listProjects(activeAgent)
      agentActivity(activeAgent).then((d) => (days = d))
      if (activeProject) {
        const p = projects.find((x) => x.id === activeProject)
        if (p) selectProject(p)
      }
    })
    return () => un.then((f) => f())
  })
</script>

<div class="shell">
  <aside class="rail">
    <div class="brand">
      <span class="dot"></span>
      Agent Workbench
      <span class="ro" title="本应用以只读方式访问 agent 数据，不会写入任何 agent 工作区">只读</span>
    </div>

    <input
      class="search"
      type="search"
      placeholder="搜索会话 / 文件 / 项目"
      bind:value={query}
      oninput={onQueryInput}
    />

    {#each agents as agent (agent.id)}
      <button
        class="agent"
        class:active={activeAgent === agent.id && !query.trim()}
        class:off={!agent.installed}
        style="--c: var(--{agent.id})"
        onclick={() => selectAgent(agent)}
        title={agent.installed ? agent.root : `未检测到 ${agent.root}`}
      >
        <span class="tag"></span>
        <div class="meta">
          <div class="name">{agent.displayName}</div>
          <div class="sub">
            {#if agent.installed}
              {agent.projectCount} 个项目 · {agent.sessionCount} 场会话
            {:else}
              未安装
            {/if}
          </div>
        </div>
        {#if agent.installed && agent.lastActive}
          <span class="when">{relTime(agent.lastActive)}</span>
        {/if}
      </button>
    {/each}

    {#if error}
      <div class="err">{error}</div>
    {/if}
  </aside>

  <ProjectList
    {projects}
    {activeProject}
    loading={loadingProjects}
    onselect={selectProject}
  />

  <DetailPane
    agent={currentAgent}
    {outcome}
    {sessions}
    {plans}
    {days}
    {hits}
    {query}
    {searching}
    {loadingActivity}
    loading={loadingSessions}
    hasProject={!!activeProject}
    projectName={activeProjectName}
    onopen={openHit}
    {repoReport}
    {repoScanning}
    {repoError}
    onscan={scanRepo}
  />
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 210px 300px 1fr;
    height: 100vh;
  }

  .rail {
    background: var(--panel);
    border-right: 1px solid var(--line);
    padding: 10px;
    overflow-y: auto;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    font-size: 12px;
    letter-spacing: 0.02em;
    padding: 6px 6px 12px;
    color: var(--dim);
  }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--accent); }
  .ro {
    margin-left: auto;
    font-size: 10px;
    font-weight: 500;
    color: var(--dimmer);
    border: 1px solid var(--line);
    border-radius: 3px;
    padding: 1px 4px;
    cursor: help;
  }

  .search {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: 5px;
    padding: 6px 8px;
    color: var(--text);
    font: inherit;
    font-size: 12px;
    margin-bottom: 10px;
  }
  .search:focus { outline: none; border-color: var(--accent); }
  .search::placeholder { color: var(--dimmer); }

  .agent {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    border-radius: 6px;
    text-align: left;
    margin-bottom: 2px;
  }
  .agent:hover { background: var(--panel-2); }
  .agent.active { background: var(--panel-2); }
  .agent.off { opacity: 0.38; cursor: default; }
  .agent.off:hover { background: none; }

  .tag {
    width: 3px;
    height: 26px;
    border-radius: 2px;
    background: var(--c);
    flex-shrink: 0;
    opacity: 0.35;
  }
  .agent.active .tag { opacity: 1; }

  .meta { min-width: 0; flex: 1; }
  .name { font-weight: 500; }
  .sub { font-size: 11px; color: var(--dim); margin-top: 2px; }
  .when { font-size: 10px; color: var(--dimmer); flex-shrink: 0; }

  .err {
    margin-top: 12px;
    padding: 8px;
    border-radius: 5px;
    background: #3a1f24;
    color: #ffb4b4;
    font-size: 11px;
    line-height: 1.5;
    user-select: text;
  }
</style>
