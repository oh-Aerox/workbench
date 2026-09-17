import { invoke as tauriInvoke } from '@tauri-apps/api/core'

const cacheListeners = new Set()
export function onCacheChanged(listener) {
  cacheListeners.add(listener)
  return () => cacheListeners.delete(listener)
}

async function invoke(command, args) {
  try {
    return await tauriInvoke(command, args)
  } finally {
    // 查询可能更新磁盘缓存；大小查询自身不能再次触发通知。
    if (command !== 'cache_size') {
      for (const listener of cacheListeners) listener()
    }
  }
}

export const listAgents = () => invoke('list_agents')
export const listProjects = (agent) => invoke('list_projects', { agent })
export const listSessions = (agent, projectId) =>
  invoke('list_sessions', { agent, projectId })
export const projectOutcome = (agent, projectId) =>
  invoke('project_outcome', { agent, projectId })
export const projectPlans = (agent, projectId) =>
  invoke('project_plans', { agent, projectId })
export const projectDocs = (projectPath) =>
  invoke('project_docs', { projectPath })
export const projectTodos = (projectPath) =>
  invoke('project_todos', { projectPath })
export const agentActivity = (agent) => invoke('agent_activity', { agent })
export const search = (query, limit = 80) => invoke('search', { query, limit })

// 缓存位于 %LOCALAPPDATA%\AgentWorkbench\parse-cache.json，内含各会话的首条
// prompt 原文。给用户一个主动清除的入口，清完只是下次打开项目慢一点。
export const cacheSize = () => invoke('cache_size')
export const clearCache = () => invoke('clear_cache')
