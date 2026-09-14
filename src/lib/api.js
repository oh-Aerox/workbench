import { invoke } from '@tauri-apps/api/core'

export const listAgents = () => invoke('list_agents')
export const listProjects = (agent) => invoke('list_projects', { agent })
export const listSessions = (agent, projectId) =>
  invoke('list_sessions', { agent, projectId })
