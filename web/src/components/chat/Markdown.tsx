// Renders the model's markdown reply (bold, headings, lists, tables, code). Raw HTML is not rendered.
import ReactMarkdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { cn } from '@/lib/utils'

const components: Components = {
  h1: (p) => <h3 className="mt-3 mb-1 text-base font-semibold first:mt-0" {...p} />,
  h2: (p) => <h3 className="mt-3 mb-1 text-base font-semibold first:mt-0" {...p} />,
  h3: (p) => <h4 className="mt-2 mb-1 text-sm font-semibold first:mt-0" {...p} />,
  h4: (p) => <h4 className="mt-2 mb-1 text-sm font-semibold first:mt-0" {...p} />,
  h5: (p) => <h5 className="mt-2 mb-1 text-sm font-medium first:mt-0" {...p} />,
  h6: (p) => <h6 className="mt-2 mb-1 text-sm font-medium first:mt-0" {...p} />,
  p: (p) => <p className="my-1.5 first:mt-0 last:mb-0" {...p} />,
  ul: (p) => <ul className="my-1.5 list-disc space-y-0.5 pl-5" {...p} />,
  ol: (p) => <ol className="my-1.5 list-decimal space-y-0.5 pl-5" {...p} />,
  a: (p) => <a className="text-primary underline underline-offset-2" target="_blank" rel="noreferrer noopener" {...p} />,
  blockquote: (p) => <blockquote className="border-border text-muted-foreground my-1.5 border-l-2 pl-3" {...p} />,
  hr: () => <hr className="border-border my-2" />,
  pre: (p) => <pre className="bg-background my-1.5 overflow-x-auto rounded border p-2 text-xs" {...p} />,
  code: ({ className, ...p }) => (
    <code className={cn('font-mono text-[0.85em] [:not(pre)>&]:bg-background [:not(pre)>&]:rounded [:not(pre)>&]:px-1', className)} {...p} />
  ),
  table: (p) => (
    <div className="my-1.5 overflow-x-auto">
      <table className="w-full border-collapse text-xs" {...p} />
    </div>
  ),
  th: (p) => <th className="border-border border-b px-2 py-1 text-left font-medium whitespace-nowrap" {...p} />,
  td: (p) => <td className="border-border border-b px-2 py-1 whitespace-nowrap" {...p} />,
}

export function Markdown({ children, className }: { children: string; className?: string }) {
  return (
    <div className={cn('break-words', className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {children}
      </ReactMarkdown>
    </div>
  )
}
