import { WorkspaceStorage, OwnerStorage, ConfigStorage, TaskService, WorkspaceService, OwnerService } from '@taskforge/core';
import path from 'path';
import os from 'os';

function getRoot(): string {
  return process.env.TASKFORGE_ROOT || path.join(os.homedir(), '.taskforge');
}

export function getServices() {
  const root = getRoot();
  const wsStorage = new WorkspaceStorage(root);
  const ownerStorage = new OwnerStorage(root);
  const configStorage = new ConfigStorage(root);
  const taskService = new TaskService(root, wsStorage, ownerStorage, configStorage);
  const workspaceService = new WorkspaceService(wsStorage);
  const ownerService = new OwnerService(ownerStorage);
  return { root, taskService, workspaceService, ownerService, configStorage };
}
