import { z } from 'zod';

export const TaskStatus = z.enum(['open', 'in_progress', 'in_review', 'done', 'archived']);
export type TaskStatus = z.infer<typeof TaskStatus>;

export const ReviewOutcome = z.enum(['approved', 'rejected']);
export type ReviewOutcome = z.infer<typeof ReviewOutcome>;

export const OwnerType = z.enum(['human', 'agent']);
export type OwnerType = z.infer<typeof OwnerType>;

export const RecurrenceFrequency = z.enum(['hourly', 'daily', 'weekly', 'monthly']);
export type RecurrenceFrequency = z.infer<typeof RecurrenceFrequency>;

export const RecurrenceSchema = z.object({
  frequency: RecurrenceFrequency,
  interval: z.number().int().positive().default(1),
  preserve_review_required: z.boolean().default(true),
  carry_forward_description: z.boolean().default(true),
  carry_forward_owner: z.boolean().default(true),
  carry_forward_due_strategy: z.enum(['none', 'relative', 'absolute']).default('none'),
});
export type Recurrence = z.infer<typeof RecurrenceSchema>;

export const TaskSchema = z.object({
  id: z.string(),
  title: z.string().min(1),
  status: TaskStatus,
  workspace: z.string(),
  owner: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
  review_required: z.boolean().default(false),
  soft_deleted: z.boolean().default(false),
  archived: z.boolean().default(false),
  description: z.string().optional().nullable(),
  due_at: z.string().optional().nullable(),
  completed_at: z.string().optional().nullable(),
  review_requested_at: z.string().optional().nullable(),
  reviewed_at: z.string().optional().nullable(),
  reviewer: z.string().optional().nullable(),
  review_outcome: ReviewOutcome.optional().nullable(),
  parent_task_id: z.string().optional().nullable(),
  blocked_by: z.array(z.string()).default([]),
  recurrence: RecurrenceSchema.optional().nullable(),
  prior_occurrence_id: z.string().optional().nullable(),
  attachment_refs: z.array(z.string()).default([]),
  artifact_refs: z.array(z.string()).default([]),
  version: z.number().int().nonnegative().default(1),
});
export type Task = z.infer<typeof TaskSchema>;

export const WorkspaceSchema = z.object({
  name: z.string(),
  created_at: z.string(),
  description: z.string().optional(),
});
export type Workspace = z.infer<typeof WorkspaceSchema>;

export const OwnerSchema = z.object({
  name: z.string(),
  type: OwnerType,
  description: z.string().optional(),
  active: z.boolean().default(true),
});
export type Owner = z.infer<typeof OwnerSchema>;

export const HookFilterSchema = z.object({
  id: z.string(),
  enabled: z.boolean().default(true),
  event: z.string(),
  command: z.string(),
  args: z.array(z.string()).default([]),
  workspace_filter: z.array(z.string()).optional(),
  owner_filter: z.array(z.string()).optional(),
  status_filter: z.array(z.string()).optional(),
  timeout_ms: z.number().optional().default(10000),
});
export type HookConfig = z.infer<typeof HookFilterSchema>;

export const ConfigSchema = z.object({
  default_workspace: z.string().default('main'),
  hooks: z.array(HookFilterSchema).default([]),
});
export type Config = z.infer<typeof ConfigSchema>;

export type HookEvent =
  | 'task.created'
  | 'subtask.created'
  | 'task.owner_changed'
  | 'task.status_changed';

export interface AuditEntry {
  ts: string;
  actor: string;
  action: string;
  task_id: string;
  [key: string]: unknown;
}

export interface HookLogEntry {
  ts: string;
  hook_id: string;
  event: string;
  task_id: string;
  ok: boolean;
  exit_code: number;
}

export interface JsonResponse<T = unknown> {
  ok: boolean;
  command: string;
  taskforge_version: string;
  data: T | null;
  warnings: Array<{ code: string; message: string }>;
  errors: Array<{ code: string; message: string }>;
}
