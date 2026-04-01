'use client';

import { useState, useEffect, useCallback } from 'react';
import { useParams, useSearchParams, useRouter } from 'next/navigation';
import Link from 'next/link';

type Task = {
  id: string;
  title: string;
  status: string;
  workspace: string;
  owner: string;
  description?: string | null;
  due_at?: string | null;
  completed_at?: string | null;
  created_at: string;
  updated_at: string;
  review_required: boolean;
  reviewer?: string | null;
  review_outcome?: string | null;
  review_requested_at?: string | null;
  reviewed_at?: string | null;
  parent_task_id?: string | null;
  blocked_by: string[];
  attachment_refs: string[];
  artifact_refs: string[];
  recurrence?: unknown;
  soft_deleted: boolean;
  archived: boolean;
  version: number;
};

type TreeData = {
  task: Task;
  subtasks: Task[];
  blockers: Task[];
};

const STATUS_COLORS: Record<string, string> = {
  open: 'bg-gray-100 text-gray-700 border-gray-300',
  in_progress: 'bg-blue-100 text-blue-700 border-blue-300',
  in_review: 'bg-amber-100 text-amber-700 border-amber-300',
  done: 'bg-green-100 text-green-700 border-green-300',
  archived: 'bg-gray-200 text-gray-500 border-gray-400',
};

function StatusBadge({ status }: { status: string }) {
  return (
    <span className={`inline-flex items-center px-2.5 py-1 rounded border text-sm font-medium ${STATUS_COLORS[status] || 'bg-gray-100 text-gray-700'}`}>
      {status.replace(/_/g, ' ')}
    </span>
  );
}

function Field({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div>
      <dt className="text-xs font-medium text-gray-500 uppercase tracking-wide">{label}</dt>
      <dd className="mt-1 text-sm text-gray-900">{value || <span className="text-gray-400">—</span>}</dd>
    </div>
  );
}

export default function TaskDetailPage() {
  const params = useParams();
  const searchParams = useSearchParams();
  const router = useRouter();
  const taskId = params.id as string;
  const workspace = searchParams.get('workspace') || 'main';

  const [tree, setTree] = useState<TreeData | null>(null);
  const [audit, setAudit] = useState<unknown[]>([]);
  const [worklog, setWorklog] = useState('');
  const [comments, setComments] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionLoading, setActionLoading] = useState(false);
  const [commentText, setCommentText] = useState('');
  const [worklogText, setWorklogText] = useState('');
  const [actor, setActor] = useState('web-user');

  const fetchData = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(`/api/tasks/${taskId}?workspace=${workspace}`);
      const data = await res.json();
      if (data.ok) {
        setTree(data.data);
        setAudit(data.data.audit || []);
        setWorklog(data.data.worklog || '');
        setComments(data.data.comments || '');
      } else {
        setError(data.errors?.[0]?.message || 'Failed to load task');
      }
    } catch {
      setError('Failed to connect to API');
    } finally {
      setLoading(false);
    }
  }, [taskId, workspace]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  const performAction = async (action: string, extra?: Record<string, unknown>) => {
    setActionLoading(true);
    try {
      const res = await fetch(`/api/tasks/${taskId}/actions`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action, workspace, actor, ...extra }),
      });
      const data = await res.json();
      if (data.ok) {
        await fetchData();
      } else {
        alert(data.errors?.[0]?.message || 'Action failed');
      }
    } catch {
      alert('Failed to connect to API');
    } finally {
      setActionLoading(false);
    }
  };

  const submitComment = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!commentText.trim()) return;
    setActionLoading(true);
    try {
      const res = await fetch(`/api/tasks/${taskId}/comments`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ workspace, actor, text: commentText }),
      });
      const data = await res.json();
      if (data.ok) {
        setCommentText('');
        await fetchData();
      } else {
        alert(data.errors?.[0]?.message || 'Failed to add comment');
      }
    } catch {
      alert('Failed to connect to API');
    } finally {
      setActionLoading(false);
    }
  };

  const submitWorklog = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!worklogText.trim()) return;
    setActionLoading(true);
    try {
      const res = await fetch(`/api/tasks/${taskId}/worklogs`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ workspace, actor, text: worklogText }),
      });
      const data = await res.json();
      if (data.ok) {
        setWorklogText('');
        await fetchData();
      } else {
        alert(data.errors?.[0]?.message || 'Failed to add worklog');
      }
    } catch {
      alert('Failed to connect to API');
    } finally {
      setActionLoading(false);
    }
  };

  if (loading) {
    return (
      <div className="p-6 flex items-center justify-center py-24">
        <div className="text-gray-400">Loading task...</div>
      </div>
    );
  }

  if (error || !tree) {
    return (
      <div className="p-6">
        <div className="bg-red-50 border border-red-200 rounded-lg p-4 text-red-700">{error || 'Task not found'}</div>
        <Link href="/tasks" className="mt-4 inline-block text-blue-600 hover:underline text-sm">
          Back to tasks
        </Link>
      </div>
    );
  }

  const { task, subtasks, blockers } = tree;

  const canStart = task.status === 'open';
  const canRequestReview = task.status === 'in_progress' && task.review_required;
  const canApproveReject = task.status === 'in_review';
  const canComplete = task.status === 'in_progress' && !task.review_required;
  const canArchive = task.status === 'done' && !task.archived;

  return (
    <div className="p-6 max-w-5xl">
      {/* Header */}
      <div className="mb-5">
        <div className="flex items-center gap-2 text-sm text-gray-500 mb-2">
          <Link href="/tasks" className="hover:text-blue-600">Tasks</Link>
          <span>/</span>
          <span className="font-mono">{task.id}</span>
        </div>
        <div className="flex items-start justify-between gap-4">
          <h2 className="text-2xl font-semibold text-gray-900 flex-1">{task.title}</h2>
          <StatusBadge status={task.status} />
        </div>
        {task.soft_deleted && (
          <span className="inline-block mt-2 px-2 py-0.5 bg-red-100 text-red-600 rounded text-xs">Soft Deleted</span>
        )}
      </div>

      <div className="grid grid-cols-3 gap-6">
        {/* Main content */}
        <div className="col-span-2 space-y-6">
          {/* Description */}
          {task.description && (
            <div className="bg-white border border-gray-200 rounded-lg p-4">
              <h3 className="text-sm font-medium text-gray-700 mb-2">Description</h3>
              <p className="text-sm text-gray-700 whitespace-pre-wrap">{task.description}</p>
            </div>
          )}

          {/* Actions */}
          <div className="bg-white border border-gray-200 rounded-lg p-4">
            <h3 className="text-sm font-medium text-gray-700 mb-3">Workflow Actions</h3>
            <div className="flex items-center gap-2 mb-3">
              <label className="text-xs text-gray-500">Actor:</label>
              <input
                type="text"
                value={actor}
                onChange={e => setActor(e.target.value)}
                className="border border-gray-300 rounded px-2 py-1 text-xs w-32"
              />
            </div>
            <div className="flex flex-wrap gap-2">
              {canStart && (
                <button
                  onClick={() => performAction('start')}
                  disabled={actionLoading}
                  className="px-3 py-1.5 bg-blue-600 text-white text-xs font-medium rounded hover:bg-blue-700 disabled:opacity-50 transition-colors"
                >
                  Start
                </button>
              )}
              {canRequestReview && (
                <button
                  onClick={() => performAction('request-review')}
                  disabled={actionLoading}
                  className="px-3 py-1.5 bg-amber-500 text-white text-xs font-medium rounded hover:bg-amber-600 disabled:opacity-50 transition-colors"
                >
                  Request Review
                </button>
              )}
              {canApproveReject && (
                <>
                  <button
                    onClick={() => performAction('approve')}
                    disabled={actionLoading}
                    className="px-3 py-1.5 bg-green-600 text-white text-xs font-medium rounded hover:bg-green-700 disabled:opacity-50 transition-colors"
                  >
                    Approve
                  </button>
                  <button
                    onClick={() => {
                      const reason = prompt('Rejection reason:');
                      if (reason) performAction('reject', { reason });
                    }}
                    disabled={actionLoading}
                    className="px-3 py-1.5 bg-red-500 text-white text-xs font-medium rounded hover:bg-red-600 disabled:opacity-50 transition-colors"
                  >
                    Reject
                  </button>
                </>
              )}
              {canComplete && (
                <button
                  onClick={() => performAction('complete')}
                  disabled={actionLoading}
                  className="px-3 py-1.5 bg-green-600 text-white text-xs font-medium rounded hover:bg-green-700 disabled:opacity-50 transition-colors"
                >
                  Complete
                </button>
              )}
              {canArchive && (
                <button
                  onClick={() => performAction('archive')}
                  disabled={actionLoading}
                  className="px-3 py-1.5 bg-gray-500 text-white text-xs font-medium rounded hover:bg-gray-600 disabled:opacity-50 transition-colors"
                >
                  Archive
                </button>
              )}
            </div>
          </div>

          {/* Subtasks */}
          {subtasks.length > 0 && (
            <div className="bg-white border border-gray-200 rounded-lg p-4">
              <h3 className="text-sm font-medium text-gray-700 mb-3">Subtasks ({subtasks.length})</h3>
              <ul className="space-y-2">
                {subtasks.map(sub => (
                  <li key={sub.id} className="flex items-center gap-3 text-sm">
                    <StatusBadge status={sub.status} />
                    <Link href={`/tasks/${sub.id}?workspace=${sub.workspace}`} className="text-gray-800 hover:text-blue-600">
                      {sub.id}: {sub.title}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Blockers */}
          {blockers.length > 0 && (
            <div className="bg-white border border-red-200 rounded-lg p-4">
              <h3 className="text-sm font-medium text-red-700 mb-3">Blocked By</h3>
              <ul className="space-y-2">
                {blockers.map(b => (
                  <li key={b.id} className="flex items-center gap-3 text-sm">
                    <StatusBadge status={b.status} />
                    <Link href={`/tasks/${b.id}?workspace=${b.workspace}`} className="text-gray-800 hover:text-blue-600">
                      {b.id}: {b.title}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Add Comment */}
          <div className="bg-white border border-gray-200 rounded-lg p-4">
            <h3 className="text-sm font-medium text-gray-700 mb-3">Add Comment</h3>
            <form onSubmit={submitComment} className="space-y-2">
              <textarea
                rows={3}
                value={commentText}
                onChange={e => setCommentText(e.target.value)}
                className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                placeholder="Write a comment..."
              />
              <button
                type="submit"
                disabled={actionLoading || !commentText.trim()}
                className="px-3 py-1.5 bg-blue-600 text-white text-xs font-medium rounded hover:bg-blue-700 disabled:opacity-50 transition-colors"
              >
                Add Comment
              </button>
            </form>
          </div>

          {/* Comments */}
          {comments && (
            <div className="bg-white border border-gray-200 rounded-lg p-4">
              <h3 className="text-sm font-medium text-gray-700 mb-3">Comments</h3>
              <pre className="text-xs text-gray-700 whitespace-pre-wrap font-sans">{comments}</pre>
            </div>
          )}

          {/* Add Worklog */}
          <div className="bg-white border border-gray-200 rounded-lg p-4">
            <h3 className="text-sm font-medium text-gray-700 mb-3">Add Worklog Entry</h3>
            <form onSubmit={submitWorklog} className="space-y-2">
              <textarea
                rows={3}
                value={worklogText}
                onChange={e => setWorklogText(e.target.value)}
                className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                placeholder="What did you work on?"
              />
              <button
                type="submit"
                disabled={actionLoading || !worklogText.trim()}
                className="px-3 py-1.5 bg-gray-700 text-white text-xs font-medium rounded hover:bg-gray-800 disabled:opacity-50 transition-colors"
              >
                Add Worklog
              </button>
            </form>
          </div>

          {/* Worklog */}
          {worklog && (
            <div className="bg-white border border-gray-200 rounded-lg p-4">
              <h3 className="text-sm font-medium text-gray-700 mb-3">Work Log</h3>
              <pre className="text-xs text-gray-700 whitespace-pre-wrap font-sans">{worklog}</pre>
            </div>
          )}

          {/* Audit Log */}
          {audit.length > 0 && (
            <div className="bg-white border border-gray-200 rounded-lg p-4">
              <h3 className="text-sm font-medium text-gray-700 mb-3">Audit Log</h3>
              <ul className="space-y-1">
                {(audit as Array<Record<string, unknown>>).map((entry, i) => (
                  <li key={i} className="text-xs text-gray-600 font-mono">
                    [{String(entry.ts)}] {String(entry.actor)} — {String(entry.action)}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>

        {/* Sidebar metadata */}
        <div className="space-y-4">
          <div className="bg-white border border-gray-200 rounded-lg p-4">
            <h3 className="text-sm font-medium text-gray-700 mb-4">Details</h3>
            <dl className="space-y-3">
              <Field label="ID" value={<span className="font-mono text-xs">{task.id}</span>} />
              <Field label="Workspace" value={task.workspace} />
              <Field label="Owner" value={task.owner} />
              <Field label="Reviewer" value={task.reviewer} />
              <Field label="Review Required" value={task.review_required ? 'Yes' : 'No'} />
              <Field label="Review Outcome" value={task.review_outcome} />
              <Field label="Due" value={task.due_at ? new Date(task.due_at).toLocaleString() : null} />
              <Field label="Completed" value={task.completed_at ? new Date(task.completed_at).toLocaleString() : null} />
              <Field label="Created" value={new Date(task.created_at).toLocaleString()} />
              <Field label="Updated" value={new Date(task.updated_at).toLocaleString()} />
              <Field label="Version" value={String(task.version)} />
              <Field label="Parent Task" value={task.parent_task_id ? (
                <Link href={`/tasks/${task.parent_task_id}?workspace=${task.workspace}`} className="text-blue-600 hover:underline font-mono text-xs">
                  {task.parent_task_id}
                </Link>
              ) : null} />
            </dl>
          </div>

          {task.attachment_refs.length > 0 && (
            <div className="bg-white border border-gray-200 rounded-lg p-4">
              <h3 className="text-sm font-medium text-gray-700 mb-2">Attachments</h3>
              <ul className="space-y-1">
                {task.attachment_refs.map((ref, i) => (
                  <li key={i} className="text-xs text-gray-600 font-mono">{ref}</li>
                ))}
              </ul>
            </div>
          )}

          {task.artifact_refs.length > 0 && (
            <div className="bg-white border border-gray-200 rounded-lg p-4">
              <h3 className="text-sm font-medium text-gray-700 mb-2">Artifacts</h3>
              <ul className="space-y-1">
                {task.artifact_refs.map((ref, i) => (
                  <li key={i} className="text-xs text-gray-600 font-mono">{ref}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
