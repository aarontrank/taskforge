import { Workspace } from '../models/types.js';
import { WorkspaceStorage } from '../storage/WorkspaceStorage.js';
import { TaskForgeError, ErrorCodes } from './errors.js';

export class WorkspaceService {
  constructor(private storage: WorkspaceStorage) {}

  async createWorkspace(name: string, description?: string): Promise<Workspace> {
    const existing = await this.storage.readWorkspace(name);
    if (existing) throw new TaskForgeError(ErrorCodes.WORKSPACE_EXISTS, `Workspace '${name}' already exists`);

    const workspace: Workspace = {
      name,
      created_at: new Date().toISOString(),
      description,
    };
    await this.storage.writeWorkspace(workspace);
    return workspace;
  }

  async getWorkspace(name: string): Promise<Workspace> {
    const ws = await this.storage.readWorkspace(name);
    if (!ws) throw new TaskForgeError(ErrorCodes.WORKSPACE_NOT_FOUND, `Workspace '${name}' not found`);
    return ws;
  }

  async listWorkspaces(): Promise<Workspace[]> {
    return this.storage.listWorkspaces();
  }
}
