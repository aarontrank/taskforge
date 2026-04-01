import fs from 'fs/promises';
import path from 'path';
import matter from 'gray-matter';
import { Task, TaskSchema } from '../models/types.js';
import {
  taskDir, tasksDir, resolveTaskDir
} from './paths.js';

export class TaskStorage {
  constructor(private root: string, private workspace: string) {}

  async readTask(taskId: string): Promise<Task | null> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) return null;

    const filePath = path.join(dir, 'task.md');
    try {
      const content = await fs.readFile(filePath, 'utf-8');
      const parsed = matter(content);
      const data = parsed.data;

      // Normalize arrays
      if (!data.blocked_by) data.blocked_by = [];
      if (!data.attachment_refs) data.attachment_refs = [];
      if (!data.artifact_refs) data.artifact_refs = [];

      return TaskSchema.parse(data);
    } catch {
      return null;
    }
  }

  async writeTask(task: Task, body?: string): Promise<void> {
    const dir = await this.getOrCreateTaskDir(task);
    const filePath = path.join(dir, 'task.md');

    // Read existing body if not provided
    if (body === undefined) {
      try {
        const existing = await fs.readFile(filePath, 'utf-8');
        const parsed = matter(existing);
        body = parsed.content.trim();
      } catch {
        body = '';
      }
    }

    const frontmatter = task as unknown as object;
    const content = matter.stringify(body || '', frontmatter);

    // Atomic write
    const tmpPath = filePath + '.tmp';
    await fs.writeFile(tmpPath, content, 'utf-8');
    await fs.rename(tmpPath, filePath);
  }

  async createTaskDir(task: Task): Promise<string> {
    return this.getOrCreateTaskDir(task);
  }

  private async getOrCreateTaskDir(task: Task): Promise<string> {
    let dir: string;
    if (task.parent_task_id) {
      dir = path.join(
        tasksDir(this.root, this.workspace),
        task.parent_task_id,
        'subtasks',
        task.id
      );
    } else {
      dir = taskDir(this.root, this.workspace, task.id);
    }

    await fs.mkdir(dir, { recursive: true });
    await fs.mkdir(path.join(dir, 'attachments'), { recursive: true });
    await fs.mkdir(path.join(dir, 'artifacts'), { recursive: true });

    if (!task.parent_task_id) {
      await fs.mkdir(path.join(dir, 'subtasks'), { recursive: true });
    }

    return dir;
  }

  async appendWorklog(taskId: string, text: string, actor: string): Promise<void> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) throw new Error(`Task ${taskId} not found`);

    const filePath = path.join(dir, 'worklog.md');
    const ts = new Date().toISOString();
    const entry = `\n## ${ts} — ${actor}\n\n${text}\n`;

    try {
      await fs.access(filePath);
      await fs.appendFile(filePath, entry, 'utf-8');
    } catch {
      await fs.writeFile(filePath, `# Work Log\n${entry}`, 'utf-8');
    }
  }

  async appendComment(taskId: string, text: string, actor: string): Promise<void> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) throw new Error(`Task ${taskId} not found`);

    const filePath = path.join(dir, 'comments.md');
    const ts = new Date().toISOString();
    const entry = `\n## ${ts} — ${actor}\n\n${text}\n`;

    try {
      await fs.access(filePath);
      await fs.appendFile(filePath, entry, 'utf-8');
    } catch {
      await fs.writeFile(filePath, `# Comments\n${entry}`, 'utf-8');
    }
  }

  async appendAudit(taskId: string, entry: Record<string, unknown>): Promise<void> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) throw new Error(`Task ${taskId} not found`);

    const filePath = path.join(dir, 'audit.log');
    const line = JSON.stringify({ ts: new Date().toISOString(), task_id: taskId, ...entry }) + '\n';
    await fs.appendFile(filePath, line, 'utf-8');
  }

  async readAudit(taskId: string): Promise<unknown[]> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) return [];

    const filePath = path.join(dir, 'audit.log');
    try {
      const content = await fs.readFile(filePath, 'utf-8');
      return content.trim().split('\n').filter(Boolean).map(l => JSON.parse(l));
    } catch {
      return [];
    }
  }

  async readWorklog(taskId: string): Promise<string> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) return '';
    const filePath = path.join(dir, 'worklog.md');
    try {
      return await fs.readFile(filePath, 'utf-8');
    } catch {
      return '';
    }
  }

  async readComments(taskId: string): Promise<string> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) return '';
    const filePath = path.join(dir, 'comments.md');
    try {
      return await fs.readFile(filePath, 'utf-8');
    } catch {
      return '';
    }
  }

  async listAllTasks(): Promise<Task[]> {
    const tasks: Task[] = [];
    const tasksDirPath = tasksDir(this.root, this.workspace);

    try {
      const entries = await fs.readdir(tasksDirPath);
      for (const entry of entries) {
        const task = await this.readTask(entry);
        if (task) {
          tasks.push(task);
          // Also read subtasks
          const subDir = path.join(tasksDirPath, entry, 'subtasks');
          try {
            const subEntries = await fs.readdir(subDir);
            for (const subEntry of subEntries) {
              const subTask = await this.readTask(subEntry);
              if (subTask) tasks.push(subTask);
            }
          } catch {}
        }
      }
    } catch {}

    return tasks;
  }

  async getNextTaskId(): Promise<string> {
    const counterPath = path.join(this.root, 'task_counter.json');
    let counter = 0;
    try {
      const data = JSON.parse(await fs.readFile(counterPath, 'utf-8'));
      counter = data.next || 0;
    } catch {}

    counter++;
    await fs.writeFile(counterPath, JSON.stringify({ next: counter }), 'utf-8');
    return `TASK-${String(counter).padStart(4, '0')}`;
  }

  async copyAttachment(taskId: string, sourcePath: string): Promise<string> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) throw new Error(`Task ${taskId} not found`);

    const attachDir = path.join(dir, 'attachments');
    await fs.mkdir(attachDir, { recursive: true });
    const filename = path.basename(sourcePath);
    const dest = path.join(attachDir, filename);
    await fs.copyFile(sourcePath, dest);
    return `attachments/${filename}`;
  }

  async copyArtifact(taskId: string, sourcePath: string): Promise<string> {
    const dir = await resolveTaskDir(this.root, this.workspace, taskId);
    if (!dir) throw new Error(`Task ${taskId} not found`);

    const artDir = path.join(dir, 'artifacts');
    await fs.mkdir(artDir, { recursive: true });
    const filename = path.basename(sourcePath);
    const dest = path.join(artDir, filename);
    await fs.copyFile(sourcePath, dest);
    return `artifacts/${filename}`;
  }
}
