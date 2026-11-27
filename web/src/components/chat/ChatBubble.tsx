import { IconMessageChatbot, IconX } from '@tabler/icons-react'
import { useQuery } from '@tanstack/react-query'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { Button } from '@/components/ui/button'
import { api, streamChat, type ChatBlock, type ChatReply, type ChatStreamEvent } from '@/lib/api'
import { BlockView } from './BlockView'

interface Message {
  role: 'user' | 'assistant'
  text: string
  blocks?: ChatBlock[]
  toolCalls?: ChatReply['tool_calls']
  error?: boolean
}

interface Streaming {
  text: string
  blocks: ChatBlock[]
  tool?: string
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
  const [stream, setStream] = useState<Streaming | null>(null)
  const abortRef = useRef<AbortController | null>(null)
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
  }, [messages, open, busy, stream])

  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false)
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open])

  // closing the chat (or unmounting) cancels an in-flight stream
  useEffect(() => {
    if (!open) abortRef.current?.abort()
  }, [open])
  useEffect(() => () => abortRef.current?.abort(), [])

  async function send(text: string) {
    const message = text.trim()
    if (!message || busy) return
    setInput('')
    setMessages((m) => [...m, { role: 'user', text: message }])
    setBusy(true)
    setStream({ text: '', blocks: [] })
    abortRef.current?.abort()
    const ctrl = new AbortController()
    abortRef.current = ctrl
    const history = messages.filter((m) => !m.error).slice(-8).map((m) => ({ role: m.role, content: m.text }))
    const acc: { text: string; blocks: ChatBlock[]; final: Message | null } = { text: '', blocks: [], final: null }
    const onEvent = (ev: ChatStreamEvent) => {
      if (ctrl.signal.aborted) return
      switch (ev.type) {
        case 'text':
          acc.text += ev.delta
          setStream((s) => ({ text: (s?.text ?? '') + ev.delta, blocks: s?.blocks ?? [] }))
          break
        case 'tool':
          setStream((s) => ({ text: s?.text ?? '', blocks: s?.blocks ?? [], tool: ev.name }))
          break
        case 'blocks':
          acc.blocks = [...acc.blocks, ...ev.blocks]
          setStream((s) => ({ text: s?.text ?? '', blocks: [...(s?.blocks ?? []), ...ev.blocks] }))
          break
        case 'reset':
          acc.text = ''
          acc.blocks = []
          setStream({ text: '', blocks: [] })
          break
        case 'done':
          acc.final = { role: 'assistant', text: ev.reply.text, blocks: ev.reply.blocks, toolCalls: ev.reply.tool_calls }
          break
        case 'error':
          acc.final = { role: 'assistant', text: ev.message, error: true }
          break
      }
    }
    try {
      await streamChat(message, history, onEvent, ctrl.signal)
      if (!ctrl.signal.aborted) {
        const msg: Message =
          acc.final ??
          (acc.text || acc.blocks.length > 0
            ? { role: 'assistant', text: acc.text, blocks: acc.blocks }
            : { role: 'assistant', text: 'The answer stream ended without a reply.', error: true })
        setMessages((m) => [...m, msg])
      }
    } catch (e) {
      if (!ctrl.signal.aborted) {
        setMessages((m) => [...m, { role: 'assistant', text: e instanceof Error ? e.message : 'Something went wrong.', error: true }])
      }
    } finally {
      if (abortRef.current === ctrl) {
        abortRef.current = null
        setStream(null)
        setBusy(false)
      }
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
            {busy && (stream && (stream.text || stream.blocks.length > 0 || stream.tool) ? (
              <div className="flex justify-start">
                <div className="bg-muted max-w-[95%] space-y-2 rounded-xl px-3 py-2 text-sm">
                  {stream.text && (
                    <p className="whitespace-pre-wrap">
                      {stream.text}
                      <span aria-hidden className="bg-foreground/50 ml-0.5 inline-block h-3.5 w-1.5 animate-pulse align-text-bottom" />
                    </p>
                  )}
                  {stream.blocks.map((b, j) => <BlockView key={j} block={b} onNavigate={() => setOpen(false)} />)}
                  {stream.tool && <div className="text-muted-foreground text-xs">Running {stream.tool}…</div>}
                </div>
              </div>
            ) : (
              <div className="text-muted-foreground text-sm">Working on it…</div>
            ))}
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
