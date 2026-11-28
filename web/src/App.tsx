import {
  IconAdjustmentsHorizontal,
  IconBuildingFactory2,
  IconCar,
  IconChartLine,
  IconCloudStorm,
  IconFileText,
  IconLayoutDashboard,
  IconLogout,
  IconMenu2,
  IconX,
  IconSettings,
  IconShoppingCart,
  IconTimeline,
  IconTopologyStar3,
  IconWorld,
} from '@tabler/icons-react'
import type { Icon } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { lazy, Suspense, useEffect, useState } from 'react'
import { NavLink, Route, Routes } from 'react-router'
import { ChatBubble } from '@/components/chat/ChatBubble'
import { Separator } from '@/components/ui/separator'
import { api } from '@/lib/api'
import { cn } from '@/lib/utils'
import Admin from '@/pages/Admin'
import BadYear from '@/pages/BadYear'
import Contracts from '@/pages/Contracts'
import Dashboard from '@/pages/Dashboard'
import Explain from '@/pages/Explain'
import Login from '@/pages/Login'
import Markets from '@/pages/Markets'
import Models from '@/pages/Models'
import Sales from '@/pages/Sales'
import Scenarios from '@/pages/Scenarios'
import Strategy from '@/pages/Strategy'
import Suppliers from '@/pages/Suppliers'

const GraphExplain = lazy(() => import('@/pages/GraphExplain'))

const nav: { to: string; label: string; icon: Icon }[] = [
  { to: '/', label: 'Dashboard', icon: IconLayoutDashboard },
  { to: '/scenarios', label: 'Scenario lab', icon: IconAdjustmentsHorizontal },
  { to: '/bad-year', label: 'Bad year', icon: IconCloudStorm },
  { to: '/explain', label: 'Explain a margin', icon: IconTimeline },
  { to: '/graph', label: 'Explain graph', icon: IconTopologyStar3 },
  { to: '/contracts', label: 'Contracts', icon: IconFileText },
  { to: '/suppliers', label: 'Suppliers', icon: IconBuildingFactory2 },
  { to: '/markets', label: 'Commodities and FX', icon: IconChartLine },
  { to: '/strategy', label: 'Markets and regulation', icon: IconWorld },
  { to: '/sales', label: 'Sales and leads', icon: IconShoppingCart },
  { to: '/models', label: 'Portfolio', icon: IconCar },
  { to: '/admin', label: 'Admin', icon: IconSettings },
]

export default function App() {
  const qc = useQueryClient()
  const [menuOpen, setMenuOpen] = useState(false)
  const session = useQuery({ queryKey: ['session'], queryFn: api.me, staleTime: Infinity, retry: false })

  // any API call that answers 401 (expired session, server restart) returns to the login page
  useEffect(() => {
    const out = () => qc.setQueryData(['session'], { auth_required: true, authenticated: false })
    window.addEventListener('vw:signed-out', out)
    return () => window.removeEventListener('vw:signed-out', out)
  }, [qc])

  useEffect(() => {
    if (!menuOpen) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setMenuOpen(false)
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [menuOpen])

  if (session.isPending) return <p className="text-muted-foreground p-6 text-sm">Loading…</p>
  if (session.data && !session.data.authenticated) return <Login />

  const signOut = async () => {
    await api.logout()
    // hard reload: drops every cached query and returns to the login page
    window.location.reload()
  }

  const sidebar = (
    <div className="flex min-h-0 flex-1 flex-col">
      <nav className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto">
        {nav.map((n) => (
          <NavLink
            key={n.to}
            to={n.to}
            end={n.to === '/'}
            onClick={() => setMenuOpen(false)}
            className={({ isActive }) =>
              cn('flex items-center gap-2 rounded-md px-3 py-2 text-sm', isActive ? 'bg-secondary font-medium' : 'hover:bg-sidebar-accent/60')
            }
          >
            <n.icon size={18} stroke={1.75} aria-hidden />
            {n.label}
          </NavLink>
        ))}
      </nav>
      {session.data?.auth_required && (
        <div className="mt-4 shrink-0 border-t pt-3">
          {session.data.user && <div className="text-muted-foreground mb-1 truncate px-3 text-xs">Signed in as {session.data.user}</div>}
          <button
            type="button"
            onClick={signOut}
            className="text-muted-foreground hover:bg-destructive/10 hover:text-destructive flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm transition-colors"
          >
            <IconLogout size={18} stroke={1.75} aria-hidden />
            Sign out
          </button>
        </div>
      )}
    </div>
  )

  return (
    <div className="flex min-h-screen flex-col md:flex-row">
      {/* mobile top bar */}
      <header className="bg-sidebar text-sidebar-foreground sticky top-0 z-40 flex items-center justify-between border-b px-4 py-2 md:hidden">
        <div className="flex items-center gap-2 font-semibold">
          <img src="/logo.png" alt="Universitas Gadjah Mada" className="h-8 w-auto dark:brightness-0 dark:invert" />
          VW Expert System
        </div>
        <button
          type="button"
          aria-label={menuOpen ? 'Close menu' : 'Open menu'}
          aria-expanded={menuOpen}
          aria-controls="mobile-nav"
          onClick={() => setMenuOpen((o) => !o)}
          className="hover:bg-sidebar-accent/60 rounded-md p-2"
        >
          {menuOpen ? <IconX size={20} aria-hidden /> : <IconMenu2 size={20} aria-hidden />}
        </button>
      </header>
      {menuOpen && (
        <>
          <div className="fixed inset-0 z-30 bg-black/40 md:hidden" onClick={() => setMenuOpen(false)} aria-hidden />
          <div id="mobile-nav" className="bg-sidebar text-sidebar-foreground fixed inset-x-0 top-[49px] z-30 max-h-[calc(100dvh-49px)] overflow-y-auto border-b p-4 shadow-lg md:hidden">
            {sidebar}
          </div>
        </>
      )}
      {/* desktop sidebar */}
      <aside className="bg-sidebar text-sidebar-foreground sticky top-0 hidden h-screen w-56 shrink-0 flex-col overflow-hidden border-r p-4 md:flex">
        <img src="/logo.png" alt="Universitas Gadjah Mada" className="mx-auto mb-3 h-20 w-auto dark:brightness-0 dark:invert" />
        <div className="mb-1 font-semibold">VW Expert System</div>
        <div className="text-muted-foreground mb-4 text-xs">synthetic demo data</div>
        <Separator className="mb-4" />
        {sidebar}
      </aside>
      <main className="min-w-0 flex-1 p-4 pb-20 md:p-6 md:pb-20">
        <Suspense fallback={<p className="text-muted-foreground text-sm">Loading…</p>}>
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/scenarios" element={<Scenarios />} />
          <Route path="/bad-year" element={<BadYear />} />
          <Route path="/explain"element={<Explain />} />
          <Route path="/explain/:model" element={<Explain />} />
          <Route path="/suppliers" element={<Suppliers />} />
          <Route path="/markets" element={<Markets />} />
          <Route path="/strategy" element={<Strategy />} />
          <Route path="/sales" element={<Sales />} />
          <Route path="/models" element={<Models />} />
          <Route path="/contracts" element={<Contracts />} />
          <Route path="/admin" element={<Admin />} />
          <Route path="/graph" element={<GraphExplain />} />
        </Routes>
        </Suspense>
      </main>
      <ChatBubble />
    </div>
  )
}
