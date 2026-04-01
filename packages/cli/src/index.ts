#!/usr/bin/env node
import { Command } from 'commander';
import {
  WorkspaceStorage,
  OwnerStorage,
  ConfigStorage,
  TaskService,
  WorkspaceService,
  OwnerService,
  TaskForgeError,
  HookEngine,
  getRootFromEnv,
  initTaskForge,
  RecurrenceSchema,
} from '@taskforge/core';
import { spawn } from 'child_process';
import path from 'path';
import { createRequire } from 'module';

const VERSION = '0.1.0';

// ─── Helpers ─────────────────────────────────────────────────────────────────

function respond<T>(
  ok: boolean,
  command: string,
  data: T | null,
  warnings: string[] = [],
  errors: Array<{ code: string; message: string }> = [],
  useJson = false,
): void {
  const envelope = {
    ok,
    command,
    taskforge_version: VERSION,
    data,
    warnings: warnings.map(w => ({ code: 'HOOK_FAILED', message: w })),
    errors,
  };

  if (useJson) {
    console.log(JSON.stringify(envelope, null, 2));
  } else {
    if (!ok) {
      for (const e of errors) {
        console.error(`Error [${e.code}]: ${e.message}`);
      }
    } else {
      if (data !== null && data !== undefined) {
        console.log(JSON.stringify(data, null, 2));
      }
    }
    for (const w of warnings) {
      console.warn(`Warning: ${w}`);
    }
  }

  if (!ok) process.exit(1);
}

function handleError(err: unknown, command: string, useJson: boolean): never {
  if (err instanceof TaskForgeError) {
    respond(false, command, null, [], [{ code: err.code, message: err.message }], useJson);
  } else {
    const msg = err instanceof Error ? err.message : String(err);
    respond(false, command, null, [], [{ code: 'UNKNOWN_ERROR', message: msg }], useJson);
  }
  process.exit(1);
}

function getServices(root: string) {
  const wsStorage = new WorkspaceStorage(root);
  const ownerStorage = new OwnerStorage(root);
  const configStorage = new ConfigStorage(root);
  const taskService = new TaskService(root, wsStorage, ownerStorage, configStorage);
  const workspaceService = new WorkspaceService(wsStorage);
  const ownerService = new OwnerService(ownerStorage);
  return { taskService, workspaceService, ownerService, configStorage };
}

function padEnd(str: string, len: number): string {
  return str.length >= len ? str.slice(0, len) : str + ' '.repeat(len - str.length);
}

function formatTable(rows: Array<Record<string, string>>, cols: string[]): string {
  if (rows.length === 0) return '(none)';
  const widths = cols.map(c => Math.max(c.length, ...rows.map(r => (r[c] || '').length)));
  const header = cols.map((c, i) => padEnd(c.toUpperCase(), widths[i])).join('  ');
  const sep = widths.map(w => '-'.repeat(w)).join('  ');
  const lines = rows.map(r => cols.map((c, i) => padEnd(r[c] || '', widths[i])).join('  '));
  return [header, sep, ...lines].join('\n');
}

// ─── Program ──────────────────────────────────────────────────────────────────

const program = new Command();

program
  .name('taskforge')
  .version(VERSION)
  .description('TaskForge — local, file-backed task management')
  .option('--root <path>', 'TaskForge root directory', getRootFromEnv())
  .option('--json', 'Output as JSON envelope');

// ─── init ─────────────────────────────────────────────────────────────────────

program
  .command('init')
  .description('Initialize a new TaskForge repository')
  .option('--root <path>', 'Override root path')
  .option('--workspace <name>', 'Default workspace name', 'main')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.opts() ?? {};
    const root = opts.root ?? globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      await initTaskForge(root, opts.workspace);
      const { workspaceService } = getServices(root);
      try {
        await workspaceService.createWorkspace(opts.workspace, 'Default workspace');
      } catch {}
      respond(true, 'init', { root, default_workspace: opts.workspace }, [], [], useJson);
    } catch (err) {
      handleError(err, 'init', useJson);
    }
  });

// ─── workspace ────────────────────────────────────────────────────────────────

const workspaceCmd = program.command('workspace').description('Manage workspaces');

workspaceCmd
  .command('create')
  .description('Create a new workspace')
  .requiredOption('--name <name>', 'Workspace name')
  .option('--description <text>', 'Workspace description')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { workspaceService } = getServices(root);
      const ws = await workspaceService.createWorkspace(opts.name, opts.description);
      respond(true, 'workspace.create', ws, [], [], useJson);
    } catch (err) {
      handleError(err, 'workspace.create', useJson);
    }
  });

workspaceCmd
  .command('list')
  .description('List all workspaces')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { workspaceService } = getServices(root);
      const workspaces = await workspaceService.listWorkspaces();
      if (!useJson) {
        const rows = workspaces.map(w => ({ name: w.name, created_at: w.created_at, description: w.description || '' }));
        console.log(formatTable(rows, ['name', 'created_at', 'description']));
      } else {
        respond(true, 'workspace.list', workspaces, [], [], useJson);
      }
    } catch (err) {
      handleError(err, 'workspace.list', useJson);
    }
  });

workspaceCmd
  .command('show')
  .description('Show workspace details')
  .requiredOption('--name <name>', 'Workspace name')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { workspaceService } = getServices(root);
      const ws = await workspaceService.getWorkspace(opts.name);
      respond(true, 'workspace.show', ws, [], [], useJson);
    } catch (err) {
      handleError(err, 'workspace.show', useJson);
    }
  });

// ─── owner ────────────────────────────────────────────────────────────────────

const ownerCmd = program.command('owner').description('Manage owners');

ownerCmd
  .command('add')
  .description('Add a new owner')
  .requiredOption('--name <name>', 'Owner name')
  .requiredOption('--type <type>', 'Owner type: human or agent')
  .option('--description <text>', 'Owner description')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { ownerService } = getServices(root);
      if (opts.type !== 'human' && opts.type !== 'agent') {
        respond(false, 'owner.add', null, [], [{ code: 'VALIDATION_ERROR', message: 'Type must be human or agent' }], useJson);
        return;
      }
      const owner = await ownerService.addOwner(opts.name, opts.type as 'human' | 'agent', opts.description);
      respond(true, 'owner.add', owner, [], [], useJson);
    } catch (err) {
      handleError(err, 'owner.add', useJson);
    }
  });

ownerCmd
  .command('list')
  .description('List all owners')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { ownerService } = getServices(root);
      const owners = await ownerService.listOwners();
      if (!useJson) {
        const rows = owners.map(o => ({
          name: o.name,
          type: o.type,
          active: o.active ? 'yes' : 'no',
          description: o.description || '',
        }));
        console.log(formatTable(rows, ['name', 'type', 'active', 'description']));
      } else {
        respond(true, 'owner.list', owners, [], [], useJson);
      }
    } catch (err) {
      handleError(err, 'owner.list', useJson);
    }
  });

ownerCmd
  .command('show')
  .description('Show owner details')
  .requiredOption('--name <name>', 'Owner name')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { ownerService } = getServices(root);
      const owner = await ownerService.getOwner(opts.name);
      respond(true, 'owner.show', owner, [], [], useJson);
    } catch (err) {
      handleError(err, 'owner.show', useJson);
    }
  });

ownerCmd
  .command('deactivate')
  .description('Deactivate an owner')
  .requiredOption('--name <name>', 'Owner name')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { ownerService } = getServices(root);
      await ownerService.deactivateOwner(opts.name);
      respond(true, 'owner.deactivate', { name: opts.name, active: false }, [], [], useJson);
    } catch (err) {
      handleError(err, 'owner.deactivate', useJson);
    }
  });

// ─── task ─────────────────────────────────────────────────────────────────────

const taskCmd = program.command('task').description('Manage tasks');

taskCmd
  .command('create')
  .description('Create a new task')
  .requiredOption('--title <title>', 'Task title')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--owner <owner>', 'Task owner')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--description <text>', 'Task description')
  .option('--reviewer <reviewer>', 'Assigned reviewer')
  .option('--due-at <iso>', 'Due date (ISO 8601)')
  .option('--parent <id>', 'Parent task ID (creates subtask)')
  .option('--review-required <bool>', 'Require review before completion (true/false)', 'false')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const reviewRequired = opts.reviewRequired === 'true' || opts.reviewRequired === '1';
      const { task, warnings } = await taskService.createTask({
        title: opts.title,
        description: opts.description,
        owner: opts.owner,
        workspace: opts.workspace,
        reviewRequired,
        reviewer: opts.reviewer,
        dueAt: opts.dueAt,
        parentId: opts.parent,
        actor: opts.actor,
      });
      respond(true, 'task.create', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.create', useJson);
    }
  });

taskCmd
  .command('show')
  .description('Show task details')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const task = await taskService.getTask(opts.id, opts.workspace);
      if (!useJson) {
        console.log(`Task:           ${task.id}`);
        console.log(`Title:          ${task.title}`);
        console.log(`Status:         ${task.status}`);
        console.log(`Workspace:      ${task.workspace}`);
        console.log(`Owner:          ${task.owner}`);
        console.log(`Reviewer:       ${task.reviewer || 'none'}`);
        console.log(`Review Req:     ${task.review_required}`);
        console.log(`Review Outcome: ${task.review_outcome || 'none'}`);
        console.log(`Created:        ${task.created_at}`);
        console.log(`Updated:        ${task.updated_at}`);
        console.log(`Due:            ${task.due_at || 'none'}`);
        console.log(`Completed:      ${task.completed_at || 'none'}`);
        console.log(`Parent:         ${task.parent_task_id || 'none'}`);
        console.log(`Blocked By:     ${task.blocked_by.join(', ') || 'none'}`);
        console.log(`Attachments:    ${task.attachment_refs.join(', ') || 'none'}`);
        console.log(`Artifacts:      ${task.artifact_refs.join(', ') || 'none'}`);
        console.log(`Version:        ${task.version}`);
        console.log(`Archived:       ${task.archived}`);
        console.log(`Soft Deleted:   ${task.soft_deleted}`);
        if (task.description) console.log(`\nDescription:\n${task.description}`);
        if (task.recurrence) console.log(`\nRecurrence: ${JSON.stringify(task.recurrence)}`);
      } else {
        respond(true, 'task.show', task, [], [], useJson);
      }
    } catch (err) {
      handleError(err, 'task.show', useJson);
    }
  });

taskCmd
  .command('list')
  .description('List tasks')
  .option('--workspace <workspace>', 'Filter by workspace')
  .option('--status <status>', 'Filter by status')
  .option('--owner <owner>', 'Filter by owner')
  .option('--reviewer <reviewer>', 'Filter by reviewer')
  .option('--review-required', 'Filter tasks requiring review')
  .option('--blocked', 'Filter blocked tasks')
  .option('--parent-task-id <id>', 'Filter by parent task')
  .option('--due-before <iso>', 'Filter by due date before')
  .option('--due-after <iso>', 'Filter by due date after')
  .option('--created-before <iso>', 'Filter by created date before')
  .option('--created-after <iso>', 'Filter by created date after')
  .option('--updated-before <iso>', 'Filter by updated date before')
  .option('--updated-after <iso>', 'Filter by updated date after')
  .option('--archived', 'Include archived tasks')
  .option('--soft-deleted', 'Show soft-deleted tasks')
  .option('--recurring', 'Filter recurring tasks')
  .option('--text <search>', 'Search in title/description')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const tasks = await taskService.listTasks({
        workspace: opts.workspace,
        status: opts.status,
        owner: opts.owner,
        reviewer: opts.reviewer,
        reviewRequired: opts.reviewRequired ? true : undefined,
        blocked: opts.blocked ? true : undefined,
        parentTaskId: opts.parentTaskId,
        dueBeforeIso: opts.dueBefore,
        dueAfterIso: opts.dueAfter,
        createdBeforeIso: opts.createdBefore,
        createdAfterIso: opts.createdAfter,
        updatedBeforeIso: opts.updatedBefore,
        updatedAfterIso: opts.updatedAfter,
        archived: opts.archived ? true : undefined,
        softDeleted: opts.softDeleted ? true : undefined,
        recurring: opts.recurring ? true : undefined,
        text: opts.text,
      });
      if (!useJson) {
        if (tasks.length === 0) {
          console.log('No tasks found.');
          return;
        }
        const rows = tasks.map(t => ({
          id: t.id,
          status: t.status,
          owner: t.owner,
          workspace: t.workspace,
          title: t.title.length > 60 ? t.title.slice(0, 57) + '...' : t.title,
        }));
        console.log(formatTable(rows, ['id', 'status', 'owner', 'workspace', 'title']));
      } else {
        respond(true, 'task.list', tasks, [], [], useJson);
      }
    } catch (err) {
      handleError(err, 'task.list', useJson);
    }
  });

taskCmd
  .command('search')
  .description('Search tasks by text')
  .requiredOption('--text <text>', 'Search text')
  .option('--workspace <workspace>', 'Limit to workspace')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const tasks = await taskService.searchTasks(opts.text, opts.workspace);
      respond(true, 'task.search', tasks, [], [], useJson);
    } catch (err) {
      handleError(err, 'task.search', useJson);
    }
  });

taskCmd
  .command('tree')
  .description('Show task tree (task + subtasks + blockers)')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const tree = await taskService.getTaskTree(opts.id, opts.workspace);
      respond(true, 'task.tree', tree, [], [], useJson);
    } catch (err) {
      handleError(err, 'task.tree', useJson);
    }
  });

taskCmd
  .command('set-title')
  .description('Update task title')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--title <title>', 'New title')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.setTitle(opts.id, opts.workspace, opts.title, opts.actor, opts.expectedVersion);
      respond(true, 'task.set-title', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.set-title', useJson);
    }
  });

taskCmd
  .command('set-description')
  .description('Update task description')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--description <text>', 'New description text')
  .option('--description-file <path>', 'Path to file containing description')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      let description = opts.description;
      if (opts.descriptionFile) {
        const fs = await import('fs/promises');
        description = await fs.readFile(opts.descriptionFile, 'utf-8');
      }
      if (!description) {
        respond(false, 'task.set-description', null, [], [{ code: 'VALIDATION_ERROR', message: 'Either --description or --description-file is required' }], useJson);
        return;
      }
      const { task, warnings } = await taskService.setDescription(opts.id, opts.workspace, description, opts.actor, opts.expectedVersion);
      respond(true, 'task.set-description', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.set-description', useJson);
    }
  });

taskCmd
  .command('assign')
  .description('Assign task to an owner')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--owner <owner>', 'New owner')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.assign(opts.id, opts.workspace, opts.owner, opts.actor, opts.expectedVersion);
      respond(true, 'task.assign', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.assign', useJson);
    }
  });

taskCmd
  .command('set-reviewer')
  .description('Set task reviewer')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--reviewer <reviewer>', 'Reviewer name')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.setReviewer(opts.id, opts.workspace, opts.reviewer, opts.actor, opts.expectedVersion);
      respond(true, 'task.set-reviewer', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.set-reviewer', useJson);
    }
  });

taskCmd
  .command('set-due')
  .description('Set task due date')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--due-at <iso>', 'Due date (ISO 8601)')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.setDueAt(opts.id, opts.workspace, opts.dueAt, opts.actor, opts.expectedVersion);
      respond(true, 'task.set-due', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.set-due', useJson);
    }
  });

taskCmd
  .command('set-review-required')
  .description('Set review required flag')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--value <bool>', 'true or false')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const value = opts.value === 'true' || opts.value === '1';
      const { task, warnings } = await taskService.setReviewRequired(opts.id, opts.workspace, value, opts.actor, opts.expectedVersion);
      respond(true, 'task.set-review-required', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.set-review-required', useJson);
    }
  });

taskCmd
  .command('add-blocker')
  .description('Add a blocking task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--blocked-by <blockerId>', 'Blocker task ID')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.addBlocker(opts.id, opts.workspace, opts.blockedBy, opts.actor, opts.expectedVersion);
      respond(true, 'task.add-blocker', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.add-blocker', useJson);
    }
  });

taskCmd
  .command('remove-blocker')
  .description('Remove a blocking task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--blocked-by <blockerId>', 'Blocker task ID to remove')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.removeBlocker(opts.id, opts.workspace, opts.blockedBy, opts.actor, opts.expectedVersion);
      respond(true, 'task.remove-blocker', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.remove-blocker', useJson);
    }
  });

taskCmd
  .command('add-attachment')
  .description('Add an attachment to a task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--path <path>', 'File path')
  .option('--mode <mode>', 'copy or link', 'copy')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const mode = opts.mode === 'link' ? 'link' : 'copy';
      const { task, warnings } = await taskService.addAttachment(opts.id, opts.workspace, opts.path, mode, opts.actor);
      respond(true, 'task.add-attachment', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.add-attachment', useJson);
    }
  });

taskCmd
  .command('add-artifact')
  .description('Add an artifact to a task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--path <path>', 'File path')
  .option('--mode <mode>', 'copy or link', 'copy')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const mode = opts.mode === 'link' ? 'link' : 'copy';
      const { task, warnings } = await taskService.addArtifact(opts.id, opts.workspace, opts.path, mode, opts.actor);
      respond(true, 'task.add-artifact', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.add-artifact', useJson);
    }
  });

taskCmd
  .command('add-comment')
  .description('Add a comment to a task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--text <text>', 'Comment text')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      await taskService.addComment(opts.id, opts.workspace, opts.text, opts.actor);
      respond(true, 'task.add-comment', { task_id: opts.id, actor: opts.actor }, [], [], useJson);
    } catch (err) {
      handleError(err, 'task.add-comment', useJson);
    }
  });

taskCmd
  .command('add-worklog')
  .description('Add a worklog entry to a task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--text <text>', 'Worklog text')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      await taskService.addWorklog(opts.id, opts.workspace, opts.text, opts.actor);
      respond(true, 'task.add-worklog', { task_id: opts.id, actor: opts.actor }, [], [], useJson);
    } catch (err) {
      handleError(err, 'task.add-worklog', useJson);
    }
  });

taskCmd
  .command('start')
  .description('Start a task (open -> in_progress)')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.startTask(opts.id, opts.workspace, opts.actor, opts.expectedVersion);
      respond(true, 'task.start', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.start', useJson);
    }
  });

taskCmd
  .command('request-review')
  .description('Request review (in_progress -> in_review)')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.requestReview(opts.id, opts.workspace, opts.actor, opts.expectedVersion);
      respond(true, 'task.request-review', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.request-review', useJson);
    }
  });

taskCmd
  .command('approve')
  .description('Approve a review (in_review -> done)')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.approveReview(opts.id, opts.workspace, opts.actor, opts.expectedVersion);
      respond(true, 'task.approve', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.approve', useJson);
    }
  });

taskCmd
  .command('reject')
  .description('Reject a review (in_review -> in_progress)')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--reason <reason>', 'Rejection reason')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.rejectReview(opts.id, opts.workspace, opts.reason, opts.actor, opts.expectedVersion);
      respond(true, 'task.reject', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.reject', useJson);
    }
  });

taskCmd
  .command('complete')
  .description('Complete a task (in_progress -> done)')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.completeTask(opts.id, opts.workspace, opts.actor, opts.expectedVersion);
      respond(true, 'task.complete', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.complete', useJson);
    }
  });

taskCmd
  .command('archive')
  .description('Archive a task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.archiveTask(opts.id, opts.workspace, opts.actor, opts.expectedVersion);
      respond(true, 'task.archive', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.archive', useJson);
    }
  });

taskCmd
  .command('soft-delete')
  .description('Soft-delete a task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.softDeleteTask(opts.id, opts.workspace, opts.actor, opts.expectedVersion);
      respond(true, 'task.soft-delete', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.soft-delete', useJson);
    }
  });

taskCmd
  .command('set-recurrence')
  .description('Set task recurrence')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .requiredOption('--frequency <freq>', 'hourly, daily, weekly, or monthly')
  .option('--interval <n>', 'Recurrence interval', parseInt)
  .option('--preserve-review-required', 'Preserve review_required on recurrence', true)
  .option('--carry-forward-description', 'Carry forward description', true)
  .option('--carry-forward-owner', 'Carry forward owner', true)
  .option('--carry-forward-due-strategy <strategy>', 'none, relative, or absolute', 'none')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const recurrence = RecurrenceSchema.parse({
        frequency: opts.frequency,
        interval: opts.interval ?? 1,
        preserve_review_required: opts.preserveReviewRequired !== false,
        carry_forward_description: opts.carryForwardDescription !== false,
        carry_forward_owner: opts.carryForwardOwner !== false,
        carry_forward_due_strategy: opts.carryForwardDueStrategy ?? 'none',
      });
      const { task, warnings } = await taskService.setRecurrence(opts.id, opts.workspace, recurrence, opts.actor, opts.expectedVersion);
      respond(true, 'task.set-recurrence', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.set-recurrence', useJson);
    }
  });

taskCmd
  .command('clear-recurrence')
  .description('Clear task recurrence')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--expected-version <n>', 'Expected version for optimistic locking', parseInt)
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.clearRecurrence(opts.id, opts.workspace, opts.actor, opts.expectedVersion);
      respond(true, 'task.clear-recurrence', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.clear-recurrence', useJson);
    }
  });

taskCmd
  .command('generate-next')
  .description('Generate next occurrence of a recurring task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .requiredOption('--actor <actor>', 'Actor performing the action')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const { task, warnings } = await taskService.generateNextOccurrence(opts.id, opts.workspace, opts.actor);
      respond(true, 'task.generate-next', task, warnings, [], useJson);
    } catch (err) {
      handleError(err, 'task.generate-next', useJson);
    }
  });

taskCmd
  .command('audit')
  .description('Show audit log for a task')
  .requiredOption('--id <id>', 'Task ID')
  .requiredOption('--workspace <workspace>', 'Workspace name')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { taskService } = getServices(root);
      const entries = await taskService.getAudit(opts.id, opts.workspace);
      respond(true, 'task.audit', entries, [], [], useJson);
    } catch (err) {
      handleError(err, 'task.audit', useJson);
    }
  });

// ─── hook ─────────────────────────────────────────────────────────────────────

const hookCmd = program.command('hook').description('Manage hooks');

hookCmd
  .command('list')
  .description('List all configured hooks')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { configStorage } = getServices(root);
      const config = await configStorage.read();
      if (!useJson) {
        if (config.hooks.length === 0) {
          console.log('No hooks configured.');
          return;
        }
        const rows = config.hooks.map(h => ({
          id: h.id,
          event: h.event,
          enabled: h.enabled ? 'yes' : 'no',
          command: h.command,
        }));
        console.log(formatTable(rows, ['id', 'event', 'enabled', 'command']));
      } else {
        respond(true, 'hook.list', config.hooks, [], [], useJson);
      }
    } catch (err) {
      handleError(err, 'hook.list', useJson);
    }
  });

hookCmd
  .command('validate')
  .description('Validate hook configuration')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { configStorage } = getServices(root);
      const config = await configStorage.read();
      respond(true, 'hook.validate', { valid: true, hooks: config.hooks.length }, [], [], useJson);
    } catch (err) {
      handleError(err, 'hook.validate', useJson);
    }
  });

hookCmd
  .command('test')
  .description('Test a hook by sending a synthetic payload')
  .requiredOption('--event <event>', 'Event type (e.g. task.created)')
  .requiredOption('--task-id <taskId>', 'Task ID for test payload')
  .option('--workspace <workspace>', 'Workspace for test payload', 'main')
  .option('--json', 'Output JSON envelope')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.parent?.opts() ?? {};
    const root = globalOpts.root ?? getRootFromEnv();
    const useJson = opts.json ?? globalOpts.json ?? false;
    try {
      const { configStorage } = getServices(root);
      const config = await configStorage.read();
      const matchingHooks = config.hooks.filter(h => h.event === opts.event && h.enabled);
      if (matchingHooks.length === 0) {
        respond(false, 'hook.test', null, [], [{ code: 'HOOK_NOT_FOUND', message: `No enabled hooks found for event '${opts.event}'` }], useJson);
        return;
      }
      const engine = new HookEngine(root, matchingHooks);
      const results = await engine.emit(opts.event as any, {
        event: opts.event as any,
        timestamp: new Date().toISOString(),
        taskforge_version: '0.1.0',
        workspace: opts.workspace,
        actor: 'test',
        task: { id: opts.taskId, title: 'Test Task', status: 'open' },
      }, opts.workspace);
      respond(true, 'hook.test', results, [], [], useJson);
    } catch (err) {
      handleError(err, 'hook.test', useJson);
    }
  });

// ─── web ──────────────────────────────────────────────────────────────────────

program
  .command('web')
  .description('Start the TaskForge web UI')
  .option('--port <port>', 'Port to listen on', '3847')
  .option('--root <path>', 'Override root path')
  .action(async (opts, cmd) => {
    const globalOpts = cmd.parent?.opts() ?? {};
    const root = opts.root ?? globalOpts.root ?? getRootFromEnv();
    const port = opts.port;

    // Verify .taskforge directory exists
    const fs = await import('fs');
    if (!fs.existsSync(root)) {
      console.error(`Error: TaskForge root not found at ${root}`);
      console.error('Run "taskforge init" first to create a TaskForge repository.');
      process.exit(1);
    }

    // Resolve the @taskforge/web package directory
    let webPkgDir: string;
    try {
      // Use __filename for CJS bundle compatibility (esbuild polyfills it)
      const req = createRequire(__filename);
      const webPkgJson = req.resolve('@taskforge/web/package.json');
      webPkgDir = path.dirname(webPkgJson);
    } catch {
      console.error('Error: Could not find @taskforge/web package.');
      console.error('Make sure @taskforge/web is installed (npm install in the taskforge repo).');
      process.exit(1);
    }

    // Resolve the next binary
    let nextBin: string;
    try {
      const req = createRequire(path.join(webPkgDir, 'index.js'));
      nextBin = req.resolve('next/dist/bin/next');
    } catch {
      // Fallback: try npx-style resolution
      nextBin = path.join(webPkgDir, 'node_modules', '.bin', 'next');
    }

    console.log(`Starting TaskForge Web UI...`);
    console.log(`  Root:  ${root}`);
    console.log(`  Port:  ${port}`);
    console.log(`  URL:   http://localhost:${port}`);
    console.log();

    const child = spawn(process.execPath, [nextBin, 'dev', '-p', port], {
      cwd: webPkgDir,
      stdio: 'inherit',
      env: {
        ...process.env,
        TASKFORGE_ROOT: root,
        PORT: port,
      },
    });

    child.on('error', (err) => {
      console.error(`Failed to start web server: ${err.message}`);
      process.exit(1);
    });

    child.on('exit', (code) => {
      process.exit(code ?? 0);
    });

    // Forward termination signals to the child
    const signals: NodeJS.Signals[] = ['SIGINT', 'SIGTERM'];
    for (const sig of signals) {
      process.on(sig, () => {
        child.kill(sig);
      });
    }
  });

// ─── Parse ────────────────────────────────────────────────────────────────────

program.parse(process.argv);
