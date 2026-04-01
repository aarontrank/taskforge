import path from 'path';

export function getDefaultRoot(): string {
  return path.join(process.cwd(), '.taskforge');
}

export function getRootFromEnv(): string {
  return process.env.TASKFORGE_ROOT || getDefaultRoot();
}

export function configPath(root: string): string {
  return path.join(root, 'config.json');
}

export function ownersPath(root: string): string {
  return path.join(root, 'owners.json');
}

export function workspacesDir(root: string): string {
  return path.join(root, 'workspaces');
}

export function workspacePath(root: string, workspace: string): string {
  return path.join(root, 'workspaces', workspace);
}

export function workspaceConfigPath(root: string, workspace: string): string {
  return path.join(workspacePath(root, workspace), 'workspace.json');
}

export function tasksDir(root: string, workspace: string): string {
  return path.join(workspacePath(root, workspace), 'tasks');
}

export function taskDir(root: string, workspace: string, taskId: string): string {
  return path.join(tasksDir(root, workspace), taskId);
}

export function taskFilePath(root: string, workspace: string, taskId: string): string {
  return path.join(taskDir(root, workspace, taskId), 'task.md');
}

export function worklogPath(root: string, workspace: string, taskId: string): string {
  return path.join(taskDir(root, workspace, taskId), 'worklog.md');
}

export function commentsPath(root: string, workspace: string, taskId: string): string {
  return path.join(taskDir(root, workspace, taskId), 'comments.md');
}

export function auditLogPath(root: string, workspace: string, taskId: string): string {
  return path.join(taskDir(root, workspace, taskId), 'audit.log');
}

export function attachmentsDir(root: string, workspace: string, taskId: string): string {
  return path.join(taskDir(root, workspace, taskId), 'attachments');
}

export function artifactsDir(root: string, workspace: string, taskId: string): string {
  return path.join(taskDir(root, workspace, taskId), 'artifacts');
}

export function subtasksDir(root: string, workspace: string, parentId: string): string {
  return path.join(taskDir(root, workspace, parentId), 'subtasks');
}

export function subtaskDir(root: string, workspace: string, parentId: string, taskId: string): string {
  return path.join(subtasksDir(root, workspace, parentId), taskId);
}

export function hooksLogPath(root: string): string {
  return path.join(root, 'hooks.log');
}

export function archiveDir(root: string): string {
  return path.join(root, 'archive');
}

// Resolve the actual task directory, checking both top-level and subtask locations
export async function resolveTaskDir(root: string, workspace: string, taskId: string): Promise<string | null> {
  const { default: fs } = await import('fs/promises');

  // Check top-level
  const topLevel = taskDir(root, workspace, taskId);
  try {
    await fs.access(topLevel);
    return topLevel;
  } catch {}

  // Scan for subtask
  const tasksDirectory = tasksDir(root, workspace);
  try {
    const parentDirs = await fs.readdir(tasksDirectory);
    for (const parent of parentDirs) {
      const subDir = path.join(tasksDirectory, parent, 'subtasks', taskId);
      try {
        await fs.access(subDir);
        return subDir;
      } catch {}
    }
  } catch {}

  return null;
}
