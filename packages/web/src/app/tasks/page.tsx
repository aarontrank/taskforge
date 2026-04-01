'use client';

import { useState, useEffect, useCallback } from 'react';
import Link from 'next/link';

type Task = {
  id: string;
  title: string;
  status: string;
  owner: string;
  workspace: string;
  due_at?: string | null;
  review_required: boolean;
  blocked_by: string[];
  created_at: string;
};

const STATUS_COLORS: Record<string, string> = {
  open: 'bg-gray-100 text-gray-700',
  in_progress: 'bg-blue-100 text-blue-700',
  in_review: 'bg-amber-100 text-amber-700',
  done: 'bg-green-100 text-green-700',
  archived: 'bg-gray-200 text-gray-500',
};

function StatusBadge({ status }: { status: string }) {
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium ${STATUS_COLORS[status] || 'bg-gray-100 text-gray-700'}`}>
      {status.replace('_', ' ')}
    </span>
  );
}

export default function TasksPage() {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filters, setFilters] = useState({
    status: '',
    owner: '',
    workspace: '',
    text: '',
  });

  const fetchTasks = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const params = new URLSearchParams();
      if (filters.status) params.set('status', filters.status);
      if (filters.owner) params.set('owner', filters.owner);
      if (filters.workspace) params.set('workspace', filters.workspace);
      if (filters.text) params.set('text', filters.text);
      const res = await fetch(`/api/tasks?${params.toString()}`);
      const data = await res.json();
      if (data.ok) {
        setTasks(data.data || []);
      } else {
        setError(data.errors?.[0]?.message || 'Failed to load tasks');
      }
    } catch (err) {
      setError('Failed to connect to API');
    } finally {
      setLoading(false);
    }
  }, [filters]);

  useEffect(() => {
    fetchTasks();
  }, [fetchTasks]);

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <h2 className="text-xl font-semibold text-gray-900">Tasks</h2>
        <Link
          href="/tasks/new"
          className="inline-flex items-center gap-1 px-4 py-2 bg-blue-600 text-white text-sm font-medium rounded-md hover:bg-blue-700 transition-colors"
        >
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
          </svg>
          New Task
        </Link>
      </div>

      {/* Filters */}
      <div className="bg-white border border-gray-200 rounded-lg p-4 mb-5 flex flex-wrap gap-3">
        <input
          type="text"
          placeholder="Search title/description..."
          value={filters.text}
          onChange={e => setFilters(f => ({ ...f, text: e.target.value }))}
          className="border border-gray-300 rounded-md px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 w-52"
        />
        <select
          value={filters.status}
          onChange={e => setFilters(f => ({ ...f, status: e.target.value }))}
          className="border border-gray-300 rounded-md px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          <option value="">All Statuses</option>
          <option value="open">Open</option>
          <option value="in_progress">In Progress</option>
          <option value="in_review">In Review</option>
          <option value="done">Done</option>
          <option value="archived">Archived</option>
        </select>
        <input
          type="text"
          placeholder="Filter by owner..."
          value={filters.owner}
          onChange={e => setFilters(f => ({ ...f, owner: e.target.value }))}
          className="border border-gray-300 rounded-md px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 w-40"
        />
        <input
          type="text"
          placeholder="Filter by workspace..."
          value={filters.workspace}
          onChange={e => setFilters(f => ({ ...f, workspace: e.target.value }))}
          className="border border-gray-300 rounded-md px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 w-40"
        />
        <button
          onClick={() => setFilters({ status: '', owner: '', workspace: '', text: '' })}
          className="px-3 py-1.5 text-sm text-gray-600 border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
        >
          Clear
        </button>
      </div>

      {/* Task list */}
      {loading ? (
        <div className="flex items-center justify-center py-16">
          <div className="text-gray-400">Loading tasks...</div>
        </div>
      ) : error ? (
        <div className="bg-red-50 border border-red-200 rounded-lg p-4 text-red-700 text-sm">{error}</div>
      ) : tasks.length === 0 ? (
        <div className="bg-white border border-gray-200 rounded-lg p-12 text-center">
          <p className="text-gray-500 mb-3">No tasks found.</p>
          <Link href="/tasks/new" className="text-blue-600 hover:underline text-sm">
            Create your first task
          </Link>
        </div>
      ) : (
        <div className="bg-white border border-gray-200 rounded-lg overflow-hidden">
          <table className="min-w-full divide-y divide-gray-200">
            <thead className="bg-gray-50">
              <tr>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">ID</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Title</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Status</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Owner</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Workspace</th>
                <th className="px-4 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Due</th>
              </tr>
            </thead>
            <tbody className="bg-white divide-y divide-gray-100">
              {tasks.map(task => (
                <tr key={task.id} className="hover:bg-gray-50 transition-colors">
                  <td className="px-4 py-3 text-xs font-mono text-gray-500 whitespace-nowrap">
                    <Link href={`/tasks/${task.id}?workspace=${task.workspace}`} className="hover:text-blue-600">
                      {task.id}
                    </Link>
                  </td>
                  <td className="px-4 py-3 text-sm text-gray-900 max-w-xs">
                    <Link href={`/tasks/${task.id}?workspace=${task.workspace}`} className="hover:text-blue-600 line-clamp-1">
                      {task.title}
                    </Link>
                    {task.review_required && (
                      <span className="ml-2 text-xs text-amber-600">(review)</span>
                    )}
                    {task.blocked_by.length > 0 && (
                      <span className="ml-2 text-xs text-red-500">(blocked)</span>
                    )}
                  </td>
                  <td className="px-4 py-3 whitespace-nowrap">
                    <StatusBadge status={task.status} />
                  </td>
                  <td className="px-4 py-3 text-sm text-gray-600 whitespace-nowrap">{task.owner}</td>
                  <td className="px-4 py-3 text-sm text-gray-600 whitespace-nowrap">{task.workspace}</td>
                  <td className="px-4 py-3 text-sm text-gray-500 whitespace-nowrap">
                    {task.due_at ? new Date(task.due_at).toLocaleDateString() : '—'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="px-4 py-2 border-t border-gray-100 text-xs text-gray-400">
            {tasks.length} task{tasks.length !== 1 ? 's' : ''}
          </div>
        </div>
      )}
    </div>
  );
}
