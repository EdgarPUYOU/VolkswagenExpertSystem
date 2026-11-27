import {
  IconAdjustmentsHorizontal,
  IconBuildingFactory2,
  IconCar,
  IconChartLine,
  IconCloudStorm,
  IconFileText,
  IconLayoutDashboard,
  IconLogout,
  IconSettings,
  IconShoppingCart,
  IconTimeline,
  IconTopologyStar3,
  IconWorld,
} from '@tabler/icons-react'
import type { Icon } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { lazy, Suspense, useEffect } from 'react'
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
  const session = useQuery({ queryKey: ['session'], queryFn: api.me, staleTime: Infinity, retry: false })

  // any API call that answers 401 (expired session, server restart) returns to the login page
  useEffect(() => {
    const out = () => qc.setQueryData(['session'], { auth_required: true, authenticated: false })
    window.addEventListener('vw:signed-out', out)
    return () => window.removeEventListener('vw:signed-out', out)
  }, [qc])

  if (session.isPending) return <p className="text-muted-foreground p-6 text-sm">Loading…</p>
  if (session.data && !session.data.authenticated) return <Login />

  const signOut = async () => {
    await api.logout()
    qc.removeQueries({ predicate: (q) => q.queryKey[0] !== 'session' })
    qc.setQueryData(['session'], { auth_required: true, authenticated: false })
  }

  return (
    <div className="flex min-h-screen">
      <aside className="bg-sidebar text-sidebar-foreground w-56 shrink-0 border-r p-4">
        <div className="mb-1 font-semibold">VW Expert System</div>
        <div className="text-muted-foreground mb-4 text-xs">synthetic demo data</div>
        <Separator className="mb-4" />
        <nav className="flex flex-col gap-1">
          {nav.map((n) => (
            <NavLink
              key={n.to}
              to={n.to}
              end={n.to === '/'}
              className={({ isActive }) =>
                cn('flex items-center gap-2 rounded-md px-3 py-2 text-sm', isActive ? 'bg-sidebar-accent font-medium' : 'hover:bg-sidebar-accent/60')
              }
            >
              <n.icon size={18} stroke={1.75} aria-hidden />
              {n.label}
            </NavLink>
          ))}
        </nav>
        {session.data?.auth_required && (
          <button
            type="button"
            onClick={signOut}
            className="text-muted-foreground hover:bg-sidebar-accent/60 mt-4 flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm"
          >
            <IconLogout size={18} stroke={1.75} aria-hidden />
            Sign out{session.data.user ? ` (${session.data.user})` : ''}
          </button>
        )}
      </aside>
      <main className="min-w-0 flex-1 p-6">
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
