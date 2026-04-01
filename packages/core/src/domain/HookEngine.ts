import { spawn } from 'child_process';
import fs from 'fs/promises';
import { HookConfig, HookEvent, Task } from '../models/types.js';
import { hooksLogPath } from '../storage/paths.js';

export interface HookPayload {
  event: HookEvent;
  timestamp: string;
  taskforge_version: string;
  workspace: string;
  actor: string;
  task: Partial<Task>;
  parent_task?: Partial<Task>;
  change?: { field: string; old: unknown; new: unknown };
}

export class HookEngine {
  constructor(private root: string, private hooks: HookConfig[]) {}

  async emit(
    event: HookEvent,
    payload: HookPayload,
    workspace?: string,
  ): Promise<Array<{ hook_id: string; ok: boolean; exitCode?: number; message?: string }>> {
    const results: Array<{ hook_id: string; ok: boolean; exitCode?: number; message?: string }> = [];

    const matchingHooks = this.hooks.filter(h => {
      if (!h.enabled) return false;
      if (h.event !== event) return false;
      if (h.workspace_filter && workspace && !h.workspace_filter.includes(workspace)) return false;
      if (h.status_filter && payload.change?.field === 'status') {
        const newStatus = payload.change.new as string;
        if (!h.status_filter.includes(newStatus)) return false;
      }
      return true;
    });

    for (const hook of matchingHooks) {
      const result = await this.runHook(hook, payload);
      results.push(result);
      await this.logHookResult(hook.id, event, payload.task.id || '', result.ok, result.exitCode ?? -1);
    }

    return results;
  }

  private runHook(
    hook: HookConfig,
    payload: HookPayload,
  ): Promise<{ hook_id: string; ok: boolean; exitCode?: number; message?: string }> {
    return new Promise(resolve => {
      const timeoutMs = hook.timeout_ms ?? 10000;
      const parts = hook.command.split(' ');
      const cmd = parts[0] ?? '';
      const cmdArgs = [...parts.slice(1), ...(hook.args || [])];

      const child = spawn(cmd, cmdArgs, {
        stdio: ['pipe', 'pipe', 'pipe'],
        shell: false,
      });

      const payloadStr = JSON.stringify(payload);
      child.stdin.write(payloadStr);
      child.stdin.end();

      let timedOut = false;
      const timer = setTimeout(() => {
        timedOut = true;
        child.kill();
        resolve({
          hook_id: hook.id,
          ok: false,
          exitCode: -1,
          message: `Hook ${hook.id} timed out after ${timeoutMs}ms`,
        });
      }, timeoutMs);

      child.on('close', code => {
        if (!timedOut) {
          clearTimeout(timer);
          const ok = code === 0;
          resolve({
            hook_id: hook.id,
            ok,
            exitCode: code ?? -1,
            message: ok ? undefined : `Hook ${hook.id} exited with code ${code}`,
          });
        }
      });

      child.on('error', err => {
        if (!timedOut) {
          clearTimeout(timer);
          resolve({
            hook_id: hook.id,
            ok: false,
            exitCode: -1,
            message: `Hook ${hook.id} failed: ${err.message}`,
          });
        }
      });
    });
  }

  private async logHookResult(
    hookId: string,
    event: string,
    taskId: string,
    ok: boolean,
    exitCode: number,
  ): Promise<void> {
    const entry = JSON.stringify({
      ts: new Date().toISOString(),
      hook_id: hookId,
      event,
      task_id: taskId,
      ok,
      exit_code: exitCode,
    }) + '\n';
    await fs.appendFile(hooksLogPath(this.root), entry, 'utf-8').catch(() => {});
  }
}
