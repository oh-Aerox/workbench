import { invoke } from '@tauri-apps/api/core'

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
