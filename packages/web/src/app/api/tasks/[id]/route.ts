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

export async function GET(req: NextRequest, { params }: { params: { id: string } }) {
  try {
    const { taskService } = getServices();
    const workspace = req.nextUrl.searchParams.get('workspace') || 'main';
    const taskId = params.id;

    const tree = await taskService.getTaskTree(taskId, workspace);
    const audit = await taskService.getAudit(taskId, workspace);
    const worklog = await taskService.getWorklog(taskId, workspace);
    const comments = await taskService.getComments(taskId, workspace);

    return ok('tasks.show', { ...tree, audit, worklog, comments });
  } catch (err) {
    if (err instanceof TaskForgeError) {
      const status = err.code === 'TASK_NOT_FOUND' ? 404 : 400;
      return fail('tasks.show', err.code, err.message, status);
    }
    return fail('tasks.show', 'UNKNOWN_ERROR', String(err), 500);
  }
}
