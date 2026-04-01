import { NextRequest, NextResponse } from 'next/server';
import { getServices } from '@/lib/services';
import { TaskForgeError } from '@taskforge/core';

const VERSION = '0.1.0';

export async function GET(_req: NextRequest) {
  try {
    const { workspaceService } = getServices();
    const workspaces = await workspaceService.listWorkspaces();
    return NextResponse.json({
      ok: true,
      command: 'workspaces.list',
      taskforge_version: VERSION,
      data: workspaces,
      warnings: [],
      errors: [],
    });
  } catch (err) {
    if (err instanceof TaskForgeError) {
      return NextResponse.json({
        ok: false,
        command: 'workspaces.list',
        taskforge_version: VERSION,
        data: null,
        warnings: [],
        errors: [{ code: err.code, message: err.message }],
      }, { status: 400 });
    }
    return NextResponse.json({
      ok: false,
      command: 'workspaces.list',
      taskforge_version: VERSION,
      data: null,
      warnings: [],
      errors: [{ code: 'UNKNOWN_ERROR', message: String(err) }],
    }, { status: 500 });
  }
}
