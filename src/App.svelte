<script>
  import { listAgents, listProjects, listSessions, projectOutcome } from './lib/api.js'
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
  let loadingProjects = $state(false)
  let loadingSessions = $state(false)
  let error = $state(null)

  // 需求 6：先按 agent 划分。切 agent 会清掉右侧两栏，避免串数据。
  async function selectAgent(agent) {
    if (!agent.installed) return
    activeAgent = agent.id
    activeProject = null
    activeProjectName = ''
    sessions = []
    outcome = null
    projects = []
    loadingProjects = true
    try {
      projects = await listProjects(agent.id)
    } catch (e) {
      error = String(e)
    } finally {
      loadingProjects = false
    }
  }

  async function selectProject(project) {
    activeProject = project.id
    activeProjectName = project.path
    sessions = []
    outcome = null
    loadingSessions = true
    try {
      // 两个视图都要解析同一批 JSONL，一次并发取完，切 tab 就不用再等
      const [o, s] = await Promise.all([
        projectOutcome(activeAgent, project.id),
        listSessions(activeAgent, project.id),
      ])
      outcome = o
      sessions = s
    } catch (e) {
      error = String(e)
    } finally {
      loadingSessions = false
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
  })
</script>

<div class="shell">
  <aside class="rail">
    <div class="brand">
      <span class="dot"></span>
      Agent Workbench
      <span class="ro" title="本应用以只读方式访问 agent 数据，不会写入任何 agent 工作区">只读</span>
    </div>

    {#each agents as agent (agent.id)}
      <button
        class="agent"
        class:active={activeAgent === agent.id}
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
    {outcome}
    {sessions}
    loading={loadingSessions}
    hasProject={!!activeProject}
    projectName={activeProjectName}
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
    padding: 6px 6px 14px;
    color: var(--dim);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
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
