import { Task, TaskStatus, Recurrence } from '../models/types.js';
import { TaskStorage } from '../storage/TaskStorage.js';
import { WorkspaceStorage } from '../storage/WorkspaceStorage.js';
import { OwnerStorage } from '../storage/OwnerStorage.js';
import { ConfigStorage } from '../storage/ConfigStorage.js';
import { HookEngine } from './HookEngine.js';
import { TaskForgeError, ErrorCodes } from './errors.js';
import { tasksDir } from '../storage/paths.js';

export interface CreateTaskOptions {
  title: string;
  description?: string;
  owner: string;
  workspace: string;
  reviewRequired?: boolean;
  reviewer?: string;
  dueAt?: string;
  parentId?: string;
  priorOccurrenceId?: string;
  actor: string;
  body?: string;
}

export interface ListTasksOptions {
  workspace?: string;
  status?: string;
  owner?: string;
  reviewer?: string;
  reviewRequired?: boolean;
  blocked?: boolean;
  parentTaskId?: string;
  dueBeforeIso?: string;
  dueAfterIso?: string;
  createdBeforeIso?: string;
  createdAfterIso?: string;
  updatedBeforeIso?: string;
  updatedAfterIso?: string;
  completedBeforeIso?: string;
  completedAfterIso?: string;
  archived?: boolean;
  softDeleted?: boolean;
  recurring?: boolean;
  text?: string;
}

export class TaskService {
  private hookEngine: HookEngine | null = null;

  constructor(
    private root: string,
    private workspaceStorage: WorkspaceStorage,
    private ownerStorage: OwnerStorage,
    private configStorage: ConfigStorage,
  ) {}

  private storage(workspace: string): TaskStorage {
    return new TaskStorage(this.root, workspace);
  }

  private async getHookEngine(): Promise<HookEngine> {
    if (!this.hookEngine) {
      const config = await this.configStorage.read();
      this.hookEngine = new HookEngine(this.root, config.hooks);
    }
    return this.hookEngine;
  }

  async createTask(opts: CreateTaskOptions): Promise<{ task: Task; warnings: string[] }> {
    // Validate workspace
    const ws = await this.workspaceStorage.readWorkspace(opts.workspace);
    if (!ws) throw new TaskForgeError(ErrorCodes.WORKSPACE_NOT_FOUND, `Workspace '${opts.workspace}' not found`);

    // Validate parent if provided
    if (opts.parentId) {
      const parent = await this.storage(opts.workspace).readTask(opts.parentId);
      if (!parent) throw new TaskForgeError(ErrorCodes.INVALID_PARENT, `Parent task '${opts.parentId}' not found`);
      if (parent.parent_task_id) throw new TaskForgeError(ErrorCodes.SUBTASK_DEPTH_EXCEEDED, 'Only one level of subtask nesting is allowed');
    }

    const store = this.storage(opts.workspace);
    const id = await store.getNextTaskId();
    const now = new Date().toISOString();

    const task: Task = {
      id,
      title: opts.title,
      status: 'open',
      workspace: opts.workspace,
      owner: opts.owner,
      created_at: now,
      updated_at: now,
      review_required: opts.reviewRequired ?? false,
      soft_deleted: false,
      archived: false,
      description: opts.description ?? null,
      due_at: opts.dueAt ?? null,
      completed_at: null,
      review_requested_at: null,
      reviewed_at: null,
      reviewer: opts.reviewer ?? null,
      review_outcome: null,
      parent_task_id: opts.parentId ?? null,
      blocked_by: [],
      recurrence: null,
      prior_occurrence_id: opts.priorOccurrenceId ?? null,
      attachment_refs: [],
      artifact_refs: [],
      version: 1,
    };

    await store.createTaskDir(task);
    await store.writeTask(task, opts.body || this.defaultTaskBody(task));
    await store.appendAudit(id, { actor: opts.actor, action: 'create_task' });

    // Emit hooks
    const engine = await this.getHookEngine();
    const warnings: string[] = [];

    const hookPayload = {
      event: (opts.parentId ? 'subtask.created' : 'task.created') as 'task.created' | 'subtask.created',
      timestamp: now,
      taskforge_version: '0.1.0',
      workspace: opts.workspace,
      actor: opts.actor,
      task: {
        id: task.id,
        title: task.title,
        status: task.status,
        owner: task.owner,
        review_required: task.review_required,
        reviewer: task.reviewer,
        parent_task_id: task.parent_task_id,
        version: task.version,
      },
    };

    const hookResults = await engine.emit(
      opts.parentId ? 'subtask.created' : 'task.created',
      hookPayload,
      opts.workspace
    );

    // Also emit task.created for subtasks
    if (opts.parentId) {
      const taskCreatedPayload = { ...hookPayload, event: 'task.created' as const };
      const taskCreatedResults = await engine.emit('task.created', taskCreatedPayload, opts.workspace);
      hookResults.push(...taskCreatedResults);
    }

    for (const r of hookResults) {
      if (!r.ok && r.message) warnings.push(r.message);
    }

    return { task, warnings };
  }

  private defaultTaskBody(task: Task): string {
    return `## Summary\n\n${task.description || task.title}\n\n## Acceptance Criteria\n\n- [ ] \n\n## Notes\n\n`;
  }

  async getTask(taskId: string, workspace: string): Promise<Task> {
    const task = await this.storage(workspace).readTask(taskId);
    if (!task) throw new TaskForgeError(ErrorCodes.TASK_NOT_FOUND, `Task '${taskId}' not found`);
    return task;
  }

  async listTasks(opts: ListTasksOptions): Promise<Task[]> {
    const workspaces = opts.workspace
      ? [opts.workspace]
      : (await this.workspaceStorage.listWorkspaces()).map(w => w.name);

    let tasks: Task[] = [];
    for (const ws of workspaces) {
      const wsTasks = await this.storage(ws).listAllTasks();
      tasks.push(...wsTasks);
    }

    // Apply filters
    return tasks.filter(t => {
      if (opts.status && t.status !== opts.status) return false;
      if (opts.owner && t.owner !== opts.owner) return false;
      if (opts.reviewer && t.reviewer !== opts.reviewer) return false;
      if (opts.reviewRequired !== undefined && t.review_required !== opts.reviewRequired) return false;
      if (opts.parentTaskId !== undefined && t.parent_task_id !== opts.parentTaskId) return false;
      if (opts.archived !== undefined && t.archived !== opts.archived) return false;
      if (!opts.softDeleted && t.soft_deleted) return false;
      if (opts.softDeleted === true && !t.soft_deleted) return false;
      if (opts.recurring !== undefined) {
        const isRecurring = !!t.recurrence;
        if (isRecurring !== opts.recurring) return false;
      }
      if (opts.blocked !== undefined) {
        const isBlocked = t.blocked_by.length > 0;
        if (isBlocked !== opts.blocked) return false;
      }
      if (opts.dueBeforeIso && t.due_at && t.due_at >= opts.dueBeforeIso) return false;
      if (opts.dueAfterIso && t.due_at && t.due_at <= opts.dueAfterIso) return false;
      if (opts.createdBeforeIso && t.created_at >= opts.createdBeforeIso) return false;
      if (opts.createdAfterIso && t.created_at <= opts.createdAfterIso) return false;
      if (opts.updatedBeforeIso && t.updated_at >= opts.updatedBeforeIso) return false;
      if (opts.updatedAfterIso && t.updated_at <= opts.updatedAfterIso) return false;
      if (opts.completedBeforeIso && t.completed_at && t.completed_at >= opts.completedBeforeIso) return false;
      if (opts.completedAfterIso && t.completed_at && t.completed_at <= opts.completedAfterIso) return false;
      if (opts.text) {
        const needle = opts.text.toLowerCase();
        const haystack = [t.title, t.description || ''].join(' ').toLowerCase();
        if (!haystack.includes(needle)) return false;
      }
      return true;
    });
  }

  async searchTasks(text: string, workspace?: string): Promise<Task[]> {
    return this.listTasks({ text, workspace, archived: false });
  }

  async getTaskTree(taskId: string, workspace: string): Promise<{ task: Task; subtasks: Task[]; blockers: Task[] }> {
    const task = await this.getTask(taskId, workspace);
    const allTasks = await this.storage(workspace).listAllTasks();

    const subtasks = allTasks.filter(t => t.parent_task_id === taskId);
    const blockers: Task[] = [];
    for (const blockerId of task.blocked_by) {
      const blocker = await this.storage(workspace).readTask(blockerId);
      if (blocker) blockers.push(blocker);
    }

    return { task, subtasks, blockers };
  }

  async setTitle(taskId: string, workspace: string, title: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, task => ({ ...task, title }));
  }

  async setDescription(taskId: string, workspace: string, description: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, task => ({ ...task, description }));
  }

  async assign(taskId: string, workspace: string, owner: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(
      taskId,
      workspace,
      actor,
      expectedVersion,
      task => ({ ...task, owner }),
      async (oldTask, newTask) => {
        if (oldTask.owner !== newTask.owner) {
          const engine = await this.getHookEngine();
          return engine.emit('task.owner_changed', {
            event: 'task.owner_changed',
            timestamp: new Date().toISOString(),
            taskforge_version: '0.1.0',
            workspace,
            actor,
            task: { id: newTask.id, title: newTask.title, status: newTask.status, version: newTask.version },
            change: { field: 'owner', old: oldTask.owner, new: newTask.owner },
          }, workspace);
        }
        return [];
      }
    );
  }

  async setReviewer(taskId: string, workspace: string, reviewer: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, task => ({ ...task, reviewer }));
  }

  async setDueAt(taskId: string, workspace: string, dueAt: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, task => ({ ...task, due_at: dueAt }));
  }

  async setReviewRequired(taskId: string, workspace: string, value: boolean, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, task => ({ ...task, review_required: value }));
  }

  async addBlocker(taskId: string, workspace: string, blockerId: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, task => ({
      ...task,
      blocked_by: [...new Set([...task.blocked_by, blockerId])],
    }));
  }

  async removeBlocker(taskId: string, workspace: string, blockerId: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, task => ({
      ...task,
      blocked_by: task.blocked_by.filter(id => id !== blockerId),
    }));
  }

  async addAttachment(taskId: string, workspace: string, filePath: string, mode: 'copy' | 'link', actor: string): Promise<{ task: Task; warnings: string[] }> {
    const store = this.storage(workspace);
    let ref: string;
    if (mode === 'copy') {
      ref = await store.copyAttachment(taskId, filePath);
    } else {
      ref = filePath;
    }
    return this.patchTask(taskId, workspace, actor, undefined, task => ({
      ...task,
      attachment_refs: [...task.attachment_refs, ref],
    }));
  }

  async addArtifact(taskId: string, workspace: string, filePath: string, mode: 'copy' | 'link', actor: string): Promise<{ task: Task; warnings: string[] }> {
    const store = this.storage(workspace);
    let ref: string;
    if (mode === 'copy') {
      ref = await store.copyArtifact(taskId, filePath);
    } else {
      ref = filePath;
    }
    return this.patchTask(taskId, workspace, actor, undefined, task => ({
      ...task,
      artifact_refs: [...task.artifact_refs, ref],
    }));
  }

  async startTask(taskId: string, workspace: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    const task = await this.getTask(taskId, workspace);

    if (task.status !== 'open') {
      throw new TaskForgeError(ErrorCodes.INVALID_STATUS_TRANSITION, `Task is ${task.status}, not open`);
    }

    // Check blockers
    for (const blockerId of task.blocked_by) {
      const blocker = await this.storage(workspace).readTask(blockerId);
      if (blocker && blocker.status !== 'done' && blocker.status !== 'archived') {
        throw new TaskForgeError(
          ErrorCodes.BLOCKERS_INCOMPLETE,
          `Task cannot start because blocker ${blockerId} is not done.`
        );
      }
    }

    return this.setStatus(taskId, workspace, 'in_progress', actor, expectedVersion);
  }

  async requestReview(taskId: string, workspace: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    const task = await this.getTask(taskId, workspace);

    if (!task.review_required) {
      throw new TaskForgeError(ErrorCodes.REVIEW_REQUIRED, 'Task does not require review');
    }
    if (task.status !== 'in_progress') {
      throw new TaskForgeError(ErrorCodes.INVALID_STATUS_TRANSITION, `Task must be in_progress to request review`);
    }

    return this.patchTask(taskId, workspace, actor, expectedVersion, t => ({
      ...t,
      status: 'in_review' as TaskStatus,
      review_requested_at: new Date().toISOString(),
    }), this.statusChangedHook('in_progress', 'in_review', workspace, actor));
  }

  async approveReview(taskId: string, workspace: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    const task = await this.getTask(taskId, workspace);

    if (task.status !== 'in_review') {
      throw new TaskForgeError(ErrorCodes.NOT_IN_REVIEW, 'Task is not in review');
    }

    const now = new Date().toISOString();
    return this.patchTask(taskId, workspace, actor, expectedVersion, t => ({
      ...t,
      status: 'done' as TaskStatus,
      reviewed_at: now,
      review_outcome: 'approved' as const,
      completed_at: now,
    }), this.statusChangedHook('in_review', 'done', workspace, actor));
  }

  async rejectReview(taskId: string, workspace: string, reason: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    const task = await this.getTask(taskId, workspace);

    if (task.status !== 'in_review') {
      throw new TaskForgeError(ErrorCodes.NOT_IN_REVIEW, 'Task is not in review');
    }

    const now = new Date().toISOString();
    const store = this.storage(workspace);

    // Append rejection as comment
    await store.appendComment(taskId, `Review rejected: ${reason}`, actor);

    return this.patchTask(taskId, workspace, actor, expectedVersion, t => ({
      ...t,
      status: 'in_progress' as TaskStatus,
      reviewed_at: now,
      review_outcome: 'rejected' as const,
    }), this.statusChangedHook('in_review', 'in_progress', workspace, actor));
  }

  async completeTask(taskId: string, workspace: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    const task = await this.getTask(taskId, workspace);

    if (task.review_required) {
      throw new TaskForgeError(ErrorCodes.REVIEW_REQUIRED, 'Task requires review before completion');
    }
    if (task.status !== 'in_progress') {
      throw new TaskForgeError(ErrorCodes.INVALID_STATUS_TRANSITION, `Task must be in_progress to complete`);
    }

    const now = new Date().toISOString();
    const result = await this.patchTask(taskId, workspace, actor, expectedVersion, t => ({
      ...t,
      status: 'done' as TaskStatus,
      completed_at: now,
    }), this.statusChangedHook('in_progress', 'done', workspace, actor));

    // Handle recurrence
    if (result.task.recurrence) {
      await this.generateNextOccurrence(taskId, workspace, actor);
    }

    return result;
  }

  async archiveTask(taskId: string, workspace: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    const task = await this.getTask(taskId, workspace);

    if (task.archived) {
      throw new TaskForgeError(ErrorCodes.ALREADY_ARCHIVED, 'Task is already archived');
    }

    return this.patchTask(taskId, workspace, actor, expectedVersion, t => ({
      ...t,
      archived: true,
      status: 'archived' as TaskStatus,
    }), this.statusChangedHook(task.status, 'archived', workspace, actor));
  }

  async softDeleteTask(taskId: string, workspace: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, t => ({
      ...t,
      soft_deleted: true,
    }));
  }

  async setRecurrence(taskId: string, workspace: string, recurrence: Recurrence, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, t => ({ ...t, recurrence }));
  }

  async clearRecurrence(taskId: string, workspace: string, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    return this.patchTask(taskId, workspace, actor, expectedVersion, t => ({ ...t, recurrence: null }));
  }

  async generateNextOccurrence(taskId: string, workspace: string, actor: string): Promise<{ task: Task; warnings: string[] }> {
    const task = await this.getTask(taskId, workspace);

    if (!task.recurrence) {
      throw new TaskForgeError(ErrorCodes.RECURRENCE_INVALID, 'Task has no recurrence configured');
    }

    const rec = task.recurrence;
    const now = new Date();
    let nextDue: Date | null = null;

    if (task.due_at && rec.carry_forward_due_strategy !== 'none') {
      const base = rec.carry_forward_due_strategy === 'relative' ? now : new Date(task.due_at);
      nextDue = this.addRecurrencePeriod(base, rec.frequency, rec.interval);
    }

    return this.createTask({
      title: task.title,
      description: rec.carry_forward_description ? (task.description ?? undefined) : undefined,
      owner: task.owner,
      workspace: task.workspace,
      reviewRequired: rec.preserve_review_required ? task.review_required : false,
      reviewer: task.reviewer ?? undefined,
      dueAt: nextDue ? nextDue.toISOString() : undefined,
      priorOccurrenceId: task.id,
      actor,
    });
  }

  private addRecurrencePeriod(base: Date, frequency: string, interval: number): Date {
    const d = new Date(base);
    switch (frequency) {
      case 'hourly': d.setHours(d.getHours() + interval); break;
      case 'daily': d.setDate(d.getDate() + interval); break;
      case 'weekly': d.setDate(d.getDate() + interval * 7); break;
      case 'monthly': d.setMonth(d.getMonth() + interval); break;
    }
    return d;
  }

  async addWorklog(taskId: string, workspace: string, text: string, actor: string): Promise<void> {
    await this.getTask(taskId, workspace);
    await this.storage(workspace).appendWorklog(taskId, text, actor);
    await this.storage(workspace).appendAudit(taskId, { actor, action: 'append_worklog' });
  }

  async addComment(taskId: string, workspace: string, text: string, actor: string): Promise<void> {
    await this.getTask(taskId, workspace);
    await this.storage(workspace).appendComment(taskId, text, actor);
    await this.storage(workspace).appendAudit(taskId, { actor, action: 'append_comment' });
  }

  async getAudit(taskId: string, workspace: string): Promise<unknown[]> {
    await this.getTask(taskId, workspace);
    return this.storage(workspace).readAudit(taskId);
  }

  async getWorklog(taskId: string, workspace: string): Promise<string> {
    await this.getTask(taskId, workspace);
    return this.storage(workspace).readWorklog(taskId);
  }

  async getComments(taskId: string, workspace: string): Promise<string> {
    await this.getTask(taskId, workspace);
    return this.storage(workspace).readComments(taskId);
  }

  private statusChangedHook(from: string, to: string, workspace: string, actor: string) {
    return async (oldTask: Task, newTask: Task) => {
      const engine = await this.getHookEngine();
      return engine.emit('task.status_changed', {
        event: 'task.status_changed',
        timestamp: new Date().toISOString(),
        taskforge_version: '0.1.0',
        workspace,
        actor,
        task: { id: newTask.id, title: newTask.title, owner: newTask.owner, review_required: newTask.review_required, reviewer: newTask.reviewer, version: newTask.version },
        change: { field: 'status', old: from, new: to },
      }, workspace);
    };
  }

  private async setStatus(taskId: string, workspace: string, status: TaskStatus, actor: string, expectedVersion?: number): Promise<{ task: Task; warnings: string[] }> {
    const oldTask = await this.getTask(taskId, workspace);
    return this.patchTask(taskId, workspace, actor, expectedVersion, t => ({
      ...t,
      status,
    }), this.statusChangedHook(oldTask.status, status, workspace, actor));
  }

  private async patchTask(
    taskId: string,
    workspace: string,
    actor: string,
    expectedVersion: number | undefined,
    mutate: (task: Task) => Task,
    postMutate?: (oldTask: Task, newTask: Task) => Promise<Array<{ hook_id: string; ok: boolean; message?: string }>>,
  ): Promise<{ task: Task; warnings: string[] }> {
    const store = this.storage(workspace);
    const oldTask = await store.readTask(taskId);
    if (!oldTask) throw new TaskForgeError(ErrorCodes.TASK_NOT_FOUND, `Task '${taskId}' not found`);

    if (oldTask.soft_deleted) {
      throw new TaskForgeError(ErrorCodes.SOFT_DELETED, `Task '${taskId}' is soft-deleted`);
    }

    if (expectedVersion !== undefined && oldTask.version !== expectedVersion) {
      throw new TaskForgeError(
        ErrorCodes.CONFLICT_VERSION_MISMATCH,
        `Version mismatch: expected ${expectedVersion}, got ${oldTask.version}`
      );
    }

    const mutated = mutate(oldTask);
    const newTask: Task = {
      ...mutated,
      updated_at: new Date().toISOString(),
      version: oldTask.version + 1,
    };

    await store.writeTask(newTask);
    await store.appendAudit(taskId, { actor, action: 'update_task' });

    const warnings: string[] = [];
    if (postMutate) {
      const results = await postMutate(oldTask, newTask);
      for (const r of results) {
        if (!r.ok && r.message) warnings.push(r.message);
      }
    }

    return { task: newTask, warnings };
  }
}
