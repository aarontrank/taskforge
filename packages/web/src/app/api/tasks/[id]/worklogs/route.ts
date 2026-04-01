import { NextRequest, NextResponse } from 'next/server';
import { getServices } from '@/lib/services';
import { TaskForgeError } from '@taskforge/core';

const VERSION = '0.1.0';

function ok<T>(command: string, data: T) {
  return NextResponse.json({
    ok: true,
    command,
    taskforge_version: VERSION,
    data,
    warnings: [],
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
    const { workspace, actor, text } = body;
    const taskId = params.id;

    if (!workspace || !actor || !text) {
      return fail('tasks.worklog', 'VALIDATION_ERROR', 'workspace, actor, and text are required');
    }

    await taskService.addWorklog(taskId, workspace, text, actor);
    return ok('tasks.worklog', { task_id: taskId, actor });
  } catch (err) {
    if (err instanceof TaskForgeError) {
      const status = err.code === 'TASK_NOT_FOUND' ? 404 : 400;
      return fail('tasks.worklog', err.code, err.message, status);
    }
    return fail('tasks.worklog', 'UNKNOWN_ERROR', String(err), 500);
  }
}
