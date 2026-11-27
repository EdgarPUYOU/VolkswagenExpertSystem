# Web front end

React + TypeScript + Vite + Tailwind + shadcn/ui (Tabler icons), TanStack Query and Table. It talks only to the Rust API under `/api`.

```bash
npm install
npm run dev      # http://localhost:5173, proxies /api to :8000 (start the API with `make dev-api`)
npm run build    # writes dist/, which the Rust binary embeds
npx tsc -b       # type check
```

Pages live in `src/pages/`, shared pieces in `src/components/`, the API client and types in `src/lib/api.ts`. From the repo root, `make dev` starts the API and this app together.
