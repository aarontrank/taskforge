import { NextRequest, NextResponse } from 'next/server';
import { getServices } from '@/lib/services';
import { TaskForgeError } from '@taskforge/core';

const VERSION = '0.1.0';

function ok<T>(command: string, data: T, warnings: string[] = []) {
  return NextResponse.json({
    ok: true,
    command,
    taskforge_version: VERSION,
    data,
    warnings: warnings.map(w => ({ code: 'HOOK_FAILED', message: w })),
    errors: [],
  });
}

function fail(command: string, code: string, message: string, status = 400) {
  return NextResponse.json({
    ok: false,
    command,
    taskforge_version: VERSION,
    data: null,
    warnings: [],
    errors: [{ code, message }],
  }, { status });
}

export async function POST(req: NextRequest, { params }: { params: { id: string } }) {
  try {
    const { taskService } = getServices();
    const body = await req.json();
    const { action, workspace, actor, reason, expected_version } = body;
    const taskId = params.id;

    if (!action || !workspace || !actor) {
      return fail('tasks.action', 'VALIDATION_ERROR', 'action, workspace, and actor are required');
    }

    let result: { task: unknown; warnings: string[] };

    switch (action) {
      case 'start':
        result = await taskService.startTask(taskId, workspace, actor, expected_version);
        break;
      case 'request-review':
        result = await taskService.requestReview(taskId, workspace, actor, expected_version);
        break;
      case 'approve':
        result = await taskService.approveReview(taskId, workspace, actor, expected_version);
        break;
      case 'reject':
        if (!reason) return fail('tasks.action', 'VALIDATION_ERROR', 'reason is required for reject');
        result = await taskService.rejectReview(taskId, workspace, reason, actor, expected_version);
        break;
      case 'complete':
        result = await taskService.completeTask(taskId, workspace, actor, expected_version);
        break;
      case 'archive':
        result = await taskService.archiveTask(taskId, workspace, actor, expected_version);
        break;
      case 'soft-delete':
        result = await taskService.softDeleteTask(taskId, workspace, actor, expected_version);
        break;
      default:
        return fail('tasks.action', 'VALIDATION_ERROR', `Unknown action: ${action}`);
    }

    return ok(`tasks.${action}`, result.task, result.warnings);
  } catch (err) {
    if (err instanceof TaskForgeError) {
      const status = err.code === 'TASK_NOT_FOUND' ? 404 : 400;
      return fail('tasks.action', err.code, err.message, status);
    }
    return fail('tasks.action', 'UNKNOWN_ERROR', String(err), 500);
  }
}
