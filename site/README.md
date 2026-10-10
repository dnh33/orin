# orin site

Static product site for `orin`, built with Astro and deployed to GitHub Pages by
`.github/workflows/site.yml`.

```
npm install   # once
npm run dev   # http://localhost:4321/orin
npm run build # writes dist/
```

The site is isolated from the Rust workspace: it has its own `package.json` and
never touches the crates. `dist/`, `node_modules/` and `.astro/` are gitignored in
this directory.

## Layout

| Section | Source |
| --- | --- |
| Hero, install command, query syntax | `src/components/*.astro` |
| Built-for-agents contract (JSON Lines, exit codes, latency, health) | `src/components/Agents.astro` |
| Benchmark charts | `src/components/Benchmarks.astro` |
| Architecture diagram | `src/components/HowItWorks.astro` |
| Benchmark data | `public/data/benchmarks.json` |

`astro.config.mjs` sets `base: '/orin'` because the site is published as a project
site. Every internal URL and the benchmark fetch go through `import.meta.env.BASE_URL`,
so the site also works if the base changes. Drop `base` if the site is ever served
from a domain root.

## Benchmark data contract

The page fetches `<base>/data/benchmarks.json` on load. The file lives in this
repository at `site/public/data/benchmarks.json`, so publishing data is a plain
commit of that file (no rebuild logic required beyond the site pipeline).

```json
{
  "schema_version": 1,
  "generated_at": "2026-10-10T18:00:00Z",
  "runs": [
    {
      "corpus": "medium",
      "corpus_files": 50000,
      "iterations": 1000,
      "tool": "orin",
      "query": "ext:rs",
      "p50_us": 240.0,
      "p95_us": 480.0,
      "qps": 4166.7,
      "matches": 412,
      "nonzero_exits": 0,
      "tool_path": "C:/path/to/orin.exe"
    }
  ]
}
```

### Top level

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `schema_version` | number | yes | `1` for this contract |
| `generated_at` | string \| null | no | ISO 8601 UTC timestamp of the export |
| `runs` | array | yes | one entry per (corpus, query, tool); `[]` renders the empty state |

### Each entry in `runs`

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `corpus` | string | yes | corpus label, e.g. `tiny`, `medium`, `large` |
| `corpus_files` | number | recommended | total files in that corpus. When present on every run it also sizes the method row (`medium (50,000 files)`) and orders the corpus-scale chart; when absent the chart still works, ordered by first appearance |
| `iterations` | number | yes | iterations per query |
| `tool` | string | yes | tool label as timed (`orin`, `fd`, `find`, `rg`). `orin` is highlighted; any other label is fine |
| `query` | string | yes | the exact query string that was run |
| `p50_us` | number | yes | median latency, microseconds |
| `p95_us` | number | yes | 95th percentile latency, microseconds |
| `qps` | number | yes | queries per second reported by the runner |
| `matches` | number | yes | matches the query returned (evidence badge) |
| `nonzero_exits` | number | yes | timed iterations that exited non-zero (evidence badge, expected `0`) |
| `tool_path` | string | yes | absolute path of the binary that was timed |

Rows missing `corpus`, `tool`, `query`, `p50_us` or `p95_us` are ignored rather
than displayed, so a partial export degrades instead of lying.

### How the page renders it

- Method row (timing, runner, compared-against, corpus, iterations) is always
  visible, with corpus and iterations filled from the selected run.
- `corpus` and `query` selectors filter the data; the chart for
  `corpus + query` groups by `tool`.
- Left chart: p50 (solid) and p95 (outlined) bars per tool with value labels.
- Right chart: p50 against corpus size for the selected query, one line per tool.
  The y axis switches to a log scale when the spread exceeds 50x, and says so.
- Badges: matches per run, nonzero exits, iterations per query, tools compared.

### Empty state

If the file is missing, unreadable, or `runs` is empty, the section shows
"Benchmark campaign running" plus the method, and draws no charts. Never commit
estimated or placeholder numbers: an empty `runs` array is the correct payload
until a benchmark run publishes real data.

## Privacy

No analytics, no cookies, no third-party requests. Fonts are self-hosted
(`@fontsource-variable/geist`, `@fontsource-variable/geist-mono`); the only
outbound links are the repository itself.
