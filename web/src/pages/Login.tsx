import { useState, type FormEvent } from 'react'
import { Button } from '@/components/ui/button'
import { ArrowRight, Eye, EyeOff } from 'lucide-react'
import { api } from '@/lib/api'

const field =
  'border-input bg-background focus-visible:border-ring focus-visible:ring-ring/50 h-9 w-full rounded-lg border px-3 text-sm shadow-xs outline-none focus-visible:ring-3'

export default function Login() {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [showPassword, setShowPassword] = useState(false)

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      await api.login(username, password)
      // hard reload: drops every cached query and boots the app fresh with the new session cookie
      window.location.reload()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Sign in failed.')
      setPassword('')
      setBusy(false)
    }
  }

  return (
    <main className="flex min-h-screen gap-2 p-2">
      <section className="flex flex-1 flex-col items-center px-6 py-6">
        <div className="hidden items-center gap-2 lg:flex">
          <img src="/logo.png" alt="Universitas Gadjah Mada" className="h-9 w-auto dark:brightness-0 dark:invert" />
          <span className="text-sm font-semibold">VW Expert System</span>
        </div>

        <div className="flex w-full max-w-[272px] flex-1 flex-col justify-center gap-6 py-10">
          <div className="space-y-1 text-center">
            <img
              src="/logo.png"
              alt="Universitas Gadjah Mada"
              className="mx-auto mb-4 h-24 w-auto lg:hidden dark:brightness-0 dark:invert"
            />
            <h1 className="text-base font-semibold">Welcome back</h1>
            <p className="text-muted-foreground text-sm">Sign in to the Volkswagen diagnostic assistant</p>
          </div>

          <form onSubmit={submit} className="space-y-2">
            <input
              className={field}
              name="username"
              placeholder="Username"
              aria-label="Username"
              autoComplete="username"
              autoFocus
              required
              value={username}
              onChange={(e) => setUsername(e.target.value)}
            />
            <div className="relative">
              <input
                className={`${field} pr-9`}
                name="password"
                type={showPassword ? 'text' : 'password'}
                placeholder="Password"
                aria-label="Password"
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
              <button
                type="button"
                onClick={() => setShowPassword((v) => !v)}
                aria-label={showPassword ? 'Hide password' : 'Show password'}
                aria-pressed={showPassword}
                className="text-muted-foreground hover:text-foreground absolute inset-y-0 right-0 flex w-9 items-center justify-center"
              >
                {showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
              </button>
            </div>
            {error && (
              <p role="alert" className="text-destructive text-sm">
                {error}
              </p>
            )}
            <Button type="submit" className="w-full" disabled={busy}>
              {busy ? 'Signing in…' : 'Continue'}
              {!busy && <ArrowRight className="size-3.5" />}
            </Button>
          </form>
        </div>

        <p className="text-muted-foreground text-xs">v{__APP_VERSION__}</p>
      </section>

      <aside className="bg-primary text-primary-foreground relative hidden flex-1 overflow-hidden rounded-2xl lg:block">
        <img
          src="/logo.png"
          alt=""
          aria-hidden
          className="pointer-events-none absolute -right-[12%] -bottom-[8%] h-[115%] w-auto max-w-none opacity-15 brightness-0 invert dark:invert-0"
        />
        <div className="relative flex h-full flex-col items-center justify-center gap-6 p-10 text-center">
          <img src="/logo.png" alt="Universitas Gadjah Mada" className="h-[55%] max-h-[420px] w-auto brightness-0 invert dark:invert-0" />
          <p className="text-primary-foreground/80 max-w-xs text-sm">Expert-system diagnostics for Volkswagen vehicles</p>
        </div>
      </aside>
    </main>
  )
}
