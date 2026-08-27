import Link from "next/link";

export default function NotFound() {
  return (
    <main className="flex min-h-screen items-center justify-center px-4">
      <div className="text-center">
        <p className="text-sm font-semibold uppercase tracking-wide text-accent-700">404</p>
        <h1 className="mt-2 text-2xl font-semibold text-slate-900">Page not found</h1>
        <p className="mt-2 text-sm text-slate-500">
          The page does not exist or is outside what your account can see.
        </p>
        <Link href="/dashboard" className="mt-6 inline-block text-sm font-medium text-accent-700 hover:underline">
          Go to the dashboard
        </Link>
      </div>
    </main>
  );
}
