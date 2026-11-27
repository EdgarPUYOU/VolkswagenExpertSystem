import {
  IconAdjustmentsHorizontal,
  IconBuildingFactory2,
  IconCar,
  IconChartLine,
  IconFileText,
  IconLayoutDashboard,
  IconSettings,
  IconTimeline,
  IconWorld,
} from '@tabler/icons-react'
import type { Icon } from '@tabler/icons-react'
import { NavLink, Route, Routes } from 'react-router'
import { ChatBubble } from '@/components/chat/ChatBubble'
import { Separator } from '@/components/ui/separator'
import { cn } from '@/lib/utils'
import Admin from '@/pages/Admin'
import Contracts from '@/pages/Contracts'
import Dashboard from '@/pages/Dashboard'
import Explain from '@/pages/Explain'
import Markets from '@/pages/Markets'
import Models from '@/pages/Models'
import Scenarios from '@/pages/Scenarios'
import Strategy from '@/pages/Strategy'
import Suppliers from '@/pages/Suppliers'

const nav: { to: string; label: string; icon: Icon }[] = [
  { to: '/', label: 'Dashboard', icon: IconLayoutDashboard },
  { to: '/scenarios', label: 'Scenario lab', icon: IconAdjustmentsHorizontal },
  { to: '/explain', label: 'Explain a margin', icon: IconTimeline },
  { to: '/contracts', label: 'Contracts', icon: IconFileText },
  { to: '/suppliers', label: 'Suppliers', icon: IconBuildingFactory2 },
  { to: '/markets', label: 'Commodities and FX', icon: IconChartLine },
  { to: '/strategy', label: 'Markets and regulation', icon: IconWorld },
  { to: '/models', label: 'Portfolio', icon: IconCar },
  { to: '/admin', label: 'Admin', icon: IconSettings },
]

export default function App() {
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
      </aside>
      <main className="min-w-0 flex-1 p-6">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/scenarios" element={<Scenarios />} />
          <Route path="/explain" element={<Explain />} />
          <Route path="/explain/:model" element={<Explain />} />
          <Route path="/suppliers" element={<Suppliers />} />
          <Route path="/markets" element={<Markets />} />
          <Route path="/strategy" element={<Strategy />} />
          <Route path="/models" element={<Models />} />
          <Route path="/contracts" element={<Contracts />} />
          <Route path="/admin" element={<Admin />} />
        </Routes>
      </main>
      <ChatBubble />
    </div>
  )
}
