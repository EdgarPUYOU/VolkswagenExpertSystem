import { IconMessageChatbot, IconX } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { Button } from '@/components/ui/button'
import { api, type ChatBlock, type ChatReply } from '@/lib/api'
import { BlockView } from './BlockView'

interface Message {
  role: 'user' | 'assistant'
  text: string
  blocks?: ChatBlock[]
  toolCalls?: ChatReply['tool_calls']
  error?: boolean
}

const STORAGE_KEY = 'vw-chat'

function load(): Message[] {
  try {
    const raw = sessionStorage.getItem(STORAGE_KEY)
    return raw ? (JSON.parse(raw) as Message[]) : []
  } catch {
    return []
  }
}

const GREETING: Message = {
  role: 'assistant',
  text: 'Ask me about cost, margin, contracts or suppliers. I run the same analyses as the pages and show the numbers here.',
}

export function ChatBubble() {
  const [open, setOpen] = useState(false)
  const [messages, setMessages] = useState<Message[]>(load)
  const [input, setInput] = useState('')
  const [busy, setBusy] = useState(false)
  const endRef = useRef<HTMLDivElement>(null)
  const latestRef = useRef<HTMLDivElement>(null)
  const status = useQuery({ queryKey: ['chat-status'], queryFn: api.chatStatus, enabled: open })

  useEffect(() => {
    try {
      sessionStorage.setItem(STORAGE_KEY, JSON.stringify(messages))
    } catch {
      /* storage can be unavailable; the chat still works */
    }
    // show a new answer from its first line; keep the bottom in view while waiting
    if (!busy && messages.at(-1)?.role === 'assistant') latestRef.current?.scrollIntoView({ block: 'start' })
    else endRef.current?.scrollIntoView({ block: 'end' })
  }, [messages, open, busy])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open])

  async function send(text: string) {
    const message = text.trim()
    if (!message || busy) return
    setInput('')
    setMessages((m) => [...m, { role: 'user', text: message }])
    setBusy(true)
    try {
      const reply = await api.chat(message)
      setMessages((m) => [...m, { role: 'assistant', text: reply.text, blocks: reply.blocks, toolCalls: reply.tool_calls }])
    } catch (e) {
      setMessages((m) => [...m, { role: 'assistant', text: e instanceof Error ? e.message : 'Something went wrong.', error: true }])
    } finally {
      setBusy(false)
    }
  }

  const onSubmit = (e: FormEvent) => {
    e.preventDefault()
    void send(input)
  }

  const shown = messages.length === 0 ? [GREETING] : messages

  return (
    <>
      {open && (
        <div
          role="dialog"
          aria-label="Ask the data"
          className="bg-card text-card-foreground fixed right-4 bottom-20 z-50 flex h-[min(40rem,calc(100vh-7rem))] w-[min(30rem,calc(100vw-2rem))] flex-col rounded-xl border shadow-xl"
        >
          <div className="flex items-center justify-between border-b px-3 py-2">
            <div>
              <div className="text-sm font-semibold">Ask the data</div>
              <div className="text-muted-foreground text-xs">
                {status.data ? (status.data.llm ? 'Language model with tools' : 'Keyword matching, no language model connected') : ' '}
              </div>
            </div>
            <div className="flex gap-1">
              <Button variant="ghost" size="sm" disabled={messages.length === 0} onClick={() => setMessages([])}>Clear</Button>
              <Button variant="ghost" size="sm" aria-label="Close chat" onClick={() => setOpen(false)}>Close</Button>
            </div>
          </div>
          <div className="flex-1 space-y-3 overflow-y-auto p-3">
            {shown.map((m, i) => (
              <div key={i} ref={i === shown.length - 1 ? latestRef : undefined} className={m.role === 'user' ? 'flex justify-end' : 'flex justify-start'}>
                <div
                  className={
                    m.role === 'user'
                      ? 'bg-primary text-primary-foreground max-w-[85%] rounded-xl px-3 py-2 text-sm'
                      : 'bg-muted max-w-[95%] space-y-2 rounded-xl px-3 py-2 text-sm'
                  }
                >
                  <p className={m.error ? 'text-destructive whitespace-pre-wrap' : 'whitespace-pre-wrap'}>{m.text}</p>
                  {m.blocks?.map((b, j) => <BlockView key={j} block={b} onNavigate={() => setOpen(false)} />)}
                  {m.toolCalls && m.toolCalls.length > 0 && (
                    <details className="text-muted-foreground text-xs">
                      <summary className="cursor-pointer">How this was answered</summary>
                      {m.toolCalls.map((c, k) => (
                        <pre key={k} className="mt-1 overflow-x-auto rounded border p-1.5 text-[11px]">{c.name}({JSON.stringify(c.args)})</pre>
                      ))}
                    </details>
                  )}
                </div>
              </div>
            ))}
            {busy && <div className="text-muted-foreground text-sm">Working on it…</div>}
            {messages.length === 0 && status.data && (
              <div className="flex flex-wrap gap-1.5 pt-1">
                {status.data.suggestions.map((s) => (
                  <Button key={s} variant="outline" size="sm" className="h-auto py-1 text-left text-xs whitespace-normal" onClick={() => void send(s)}>
                    {s}
                  </Button>
                ))}
              </div>
            )}
            <div ref={endRef} />
          </div>
          <form onSubmit={onSubmit} className="flex gap-2 border-t p-2">
            <input
              className="border-input bg-background focus-visible:ring-ring/50 h-9 flex-1 rounded-lg border px-3 text-sm outline-none focus-visible:ring-3"
              placeholder="e.g. What if rubber goes up 15%?"
              aria-label="Your question"
              name="question"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              autoFocus
            />
            <Button type="submit" disabled={busy || input.trim() === ''}>Send</Button>
          </form>
        </div>
      )}
      <Button
        className="fixed right-4 bottom-4 z-50 h-12 rounded-full px-5 shadow-lg"
        aria-label={open ? 'Close chat' : 'Open chat'}
        onClick={() => setOpen((o) => !o)}
      >
        {open ? <IconX size={16} aria-hidden /> : <IconMessageChatbot size={16} aria-hidden />}
        {open ? 'Close' : 'Ask the data'}
      </Button>
    </>
  )
}
