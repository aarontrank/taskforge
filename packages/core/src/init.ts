import fs from 'fs/promises';
import path from 'path';
import { configPath, ownersPath, workspacesDir } from './storage/paths.js';

export async function initTaskForge(root: string, defaultWorkspace = 'main'): Promise<void> {
  await fs.mkdir(root, { recursive: true });
  await fs.mkdir(workspacesDir(root), { recursive: true });
  await fs.mkdir(path.join(root, 'archive'), { recursive: true });

  // Create config if not exists
  try {
    await fs.access(configPath(root));
  } catch {
    await fs.writeFile(
      configPath(root),
      JSON.stringify({ default_workspace: defaultWorkspace, hooks: [] }, null, 2),
      'utf-8'
    );
  }

  // Create owners if not exists
  try {
    await fs.access(ownersPath(root));
  } catch {
    await fs.writeFile(ownersPath(root), JSON.stringify([], null, 2), 'utf-8');
  }
}
