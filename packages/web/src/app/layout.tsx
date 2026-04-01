import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'TaskForge',
  description: 'Local, file-backed task management',
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body className="bg-gray-50 min-h-screen">
        <div className="flex min-h-screen">
          {/* Sidebar */}
          <aside className="w-56 bg-gray-900 text-gray-100 flex flex-col min-h-screen">
            <div className="px-4 py-5 border-b border-gray-700">
              <h1 className="text-lg font-bold tracking-tight text-white">TaskForge</h1>
              <p className="text-xs text-gray-400 mt-0.5">Local Task Manager</p>
            </div>
            <nav className="flex-1 px-2 py-4 space-y-1">
              <a
                href="/tasks"
                className="flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium text-gray-300 hover:bg-gray-700 hover:text-white transition-colors"
              >
                <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                </svg>
                Tasks
              </a>
              <a
                href="/tasks/new"
                className="flex items-center gap-2 px-3 py-2 rounded-md text-sm font-medium text-gray-300 hover:bg-gray-700 hover:text-white transition-colors"
              >
                <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
                </svg>
                New Task
              </a>
            </nav>
            <div className="px-4 py-3 border-t border-gray-700">
              <p className="text-xs text-gray-500">v0.1.0</p>
            </div>
          </aside>

          {/* Main content */}
          <main className="flex-1 overflow-auto">
            {children}
          </main>
        </div>
      </body>
    </html>
  );
}
