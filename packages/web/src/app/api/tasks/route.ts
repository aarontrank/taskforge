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

export async function GET(req: NextRequest) {
  try {
    const { taskService } = getServices();
    const sp = req.nextUrl.searchParams;
    const tasks = await taskService.listTasks({
      workspace: sp.get('workspace') || undefined,
      status: sp.get('status') || undefined,
      owner: sp.get('owner') || undefined,
      reviewer: sp.get('reviewer') || undefined,
      text: sp.get('text') || undefined,
      archived: sp.get('archived') === 'true' ? true : undefined,
      softDeleted: sp.get('soft_deleted') === 'true' ? true : undefined,
      recurring: sp.get('recurring') === 'true' ? true : undefined,
      reviewRequired: sp.get('review_required') === 'true' ? true : undefined,
    });
    return ok('tasks.list', tasks);
  } catch (err) {
    if (err instanceof TaskForgeError) {
      return fail('tasks.list', err.code, err.message);
    }
    return fail('tasks.list', 'UNKNOWN_ERROR', String(err), 500);
  }
}

export async function POST(req: NextRequest) {
  try {
    const { taskService } = getServices();
    const body = await req.json();
    const { title, description, workspace, owner, reviewer, due_at, review_required, actor, parent_id } = body;

    if (!title || !workspace || !owner || !actor) {
      return fail('tasks.create', 'VALIDATION_ERROR', 'title, workspace, owner, and actor are required');
    }

    const { task, warnings } = await taskService.createTask({
      title,
      description,
      workspace,
      owner,
      reviewer,
      dueAt: due_at,
      reviewRequired: review_required ?? false,
      parentId: parent_id,
      actor,
    });

    return ok('tasks.create', task, warnings);
  } catch (err) {
    if (err instanceof TaskForgeError) {
      return fail('tasks.create', err.code, err.message);
    }
    return fail('tasks.create', 'UNKNOWN_ERROR', String(err), 500);
  }
}
