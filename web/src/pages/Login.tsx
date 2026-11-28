import { useState, type FormEvent } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { api } from '@/lib/api'

const field =
  'border-input bg-background focus-visible:border-ring focus-visible:ring-ring/50 h-9 w-full rounded-lg border px-3 text-sm outline-none focus-visible:ring-3'

export default function Login() {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

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
    <main className="flex min-h-screen items-center justify-center p-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <img src="/logo.png" alt="Universitas Gadjah Mada" className="mx-auto mb-2 h-28 w-auto dark:brightness-0 dark:invert" />
          <CardTitle className="text-center text-lg">VW Expert System</CardTitle>
          <CardDescription className="text-center">Sign in to continue.</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submit} className="space-y-4">
            <label className="block space-y-1.5 text-sm font-medium">
              Username
              <input
                className={field}
                name="username"
                autoComplete="username"
                autoFocus
                required
                value={username}
                onChange={(e) => setUsername(e.target.value)}
              />
            </label>
            <label className="block space-y-1.5 text-sm font-medium">
              Password
              <input
                className={field}
                name="password"
                type="password"
                autoComplete="current-password"
                required
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </label>
            {error && (
              <p role="alert" className="text-destructive text-sm">
                {error}
              </p>
            )}
            <Button type="submit" className="w-full" disabled={busy}>
              {busy ? 'Signing in…' : 'Sign in'}
            </Button>
          </form>
        </CardContent>
      </Card>
    </main>
  )
}
