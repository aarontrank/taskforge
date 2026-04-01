'use client';

import { useState, useEffect } from 'react';
import { useRouter } from 'next/navigation';

type Workspace = { name: string; description?: string };
type Owner = { name: string; type: string; active: boolean };

export default function NewTaskPage() {
  const router = useRouter();
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [owners, setOwners] = useState<Owner[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [form, setForm] = useState({
    title: '',
    description: '',
    workspace: '',
    owner: '',
    reviewer: '',
    due_at: '',
    review_required: false,
    actor: 'web-user',
  });

  useEffect(() => {
    Promise.all([
      fetch('/api/workspaces').then(r => r.json()),
      fetch('/api/owners').then(r => r.json()),
    ]).then(([wsData, ownerData]) => {
      if (wsData.ok) setWorkspaces(wsData.data || []);
      if (ownerData.ok) setOwners((ownerData.data || []).filter((o: Owner) => o.active));
    }).catch(() => {});
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError(null);

    try {
      const res = await fetch('/api/tasks', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          title: form.title,
          description: form.description || undefined,
          workspace: form.workspace,
          owner: form.owner,
          reviewer: form.reviewer || undefined,
          due_at: form.due_at || undefined,
          review_required: form.review_required,
          actor: form.actor,
        }),
      });
      const data = await res.json();
      if (data.ok) {
        router.push(`/tasks/${data.data.id}?workspace=${data.data.workspace}`);
      } else {
        setError(data.errors?.[0]?.message || 'Failed to create task');
      }
    } catch {
      setError('Failed to connect to API');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="p-6 max-w-2xl">
      <div className="mb-6">
        <h2 className="text-xl font-semibold text-gray-900">New Task</h2>
        <p className="text-sm text-gray-500 mt-1">Create a new task in your workspace</p>
      </div>

      <form onSubmit={handleSubmit} className="bg-white border border-gray-200 rounded-lg p-6 space-y-5">
        {error && (
          <div className="bg-red-50 border border-red-200 rounded-md p-3 text-red-700 text-sm">{error}</div>
        )}

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Title <span className="text-red-500">*</span>
          </label>
          <input
            type="text"
            required
            value={form.title}
            onChange={e => setForm(f => ({ ...f, title: e.target.value }))}
            className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            placeholder="Task title"
          />
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">Description</label>
          <textarea
            rows={4}
            value={form.description}
            onChange={e => setForm(f => ({ ...f, description: e.target.value }))}
            className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            placeholder="Task description (optional)"
          />
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Workspace <span className="text-red-500">*</span>
            </label>
            {workspaces.length > 0 ? (
              <select
                required
                value={form.workspace}
                onChange={e => setForm(f => ({ ...f, workspace: e.target.value }))}
                className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              >
                <option value="">Select workspace</option>
                {workspaces.map(ws => (
                  <option key={ws.name} value={ws.name}>{ws.name}</option>
                ))}
              </select>
            ) : (
              <input
                type="text"
                required
                value={form.workspace}
                onChange={e => setForm(f => ({ ...f, workspace: e.target.value }))}
                className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                placeholder="Workspace name"
              />
            )}
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Owner <span className="text-red-500">*</span>
            </label>
            {owners.length > 0 ? (
              <select
                required
                value={form.owner}
                onChange={e => setForm(f => ({ ...f, owner: e.target.value }))}
                className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              >
                <option value="">Select owner</option>
                {owners.map(o => (
                  <option key={o.name} value={o.name}>{o.name} ({o.type})</option>
                ))}
              </select>
            ) : (
              <input
                type="text"
                required
                value={form.owner}
                onChange={e => setForm(f => ({ ...f, owner: e.target.value }))}
                className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                placeholder="Owner name"
              />
            )}
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">Reviewer</label>
            <input
              type="text"
              value={form.reviewer}
              onChange={e => setForm(f => ({ ...f, reviewer: e.target.value }))}
              className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
              placeholder="Reviewer name (optional)"
            />
          </div>

          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">Due Date</label>
            <input
              type="datetime-local"
              value={form.due_at}
              onChange={e => setForm(f => ({ ...f, due_at: e.target.value }))}
              className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
        </div>

        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">Actor</label>
          <input
            type="text"
            required
            value={form.actor}
            onChange={e => setForm(f => ({ ...f, actor: e.target.value }))}
            className="w-full border border-gray-300 rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            placeholder="Your name or identifier"
          />
        </div>

        <div className="flex items-center gap-2">
          <input
            type="checkbox"
            id="review_required"
            checked={form.review_required}
            onChange={e => setForm(f => ({ ...f, review_required: e.target.checked }))}
            className="h-4 w-4 text-blue-600 border-gray-300 rounded"
          />
          <label htmlFor="review_required" className="text-sm text-gray-700">
            Review required before completion
          </label>
        </div>

        <div className="flex gap-3 pt-2">
          <button
            type="submit"
            disabled={loading}
            className="px-4 py-2 bg-blue-600 text-white text-sm font-medium rounded-md hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            {loading ? 'Creating...' : 'Create Task'}
          </button>
          <button
            type="button"
            onClick={() => router.back()}
            className="px-4 py-2 border border-gray-300 text-gray-700 text-sm font-medium rounded-md hover:bg-gray-50 transition-colors"
          >
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
}
