import fs from 'fs/promises';
import { Workspace, WorkspaceSchema } from '../models/types.js';
import { workspacesDir, workspaceConfigPath, workspacePath, tasksDir } from './paths.js';

export class WorkspaceStorage {
  constructor(private root: string) {}

  async readWorkspace(name: string): Promise<Workspace | null> {
    try {
      const content = await fs.readFile(workspaceConfigPath(this.root, name), 'utf-8');
      return WorkspaceSchema.parse(JSON.parse(content));
    } catch {
      return null;
    }
  }

  async writeWorkspace(workspace: Workspace): Promise<void> {
    const dir = workspacePath(this.root, workspace.name);
    await fs.mkdir(dir, { recursive: true });
    await fs.mkdir(tasksDir(this.root, workspace.name), { recursive: true });
    const filePath = workspaceConfigPath(this.root, workspace.name);
    await fs.writeFile(filePath, JSON.stringify(workspace, null, 2), 'utf-8');
  }

  async listWorkspaces(): Promise<Workspace[]> {
    const dir = workspacesDir(this.root);
    try {
      const entries = await fs.readdir(dir);
      const workspaces: Workspace[] = [];
      for (const entry of entries) {
        const ws = await this.readWorkspace(entry);
        if (ws) workspaces.push(ws);
      }
      return workspaces;
    } catch {
      return [];
    }
  }
}
