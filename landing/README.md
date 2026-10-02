# vidcapture landing

Source for <https://vidcapture.elvisbrevi.cl>, served by Cloudflare Pages like
the other `*.elvisbrevi.cl` side projects.

Plain HTML, CSS and a few lines of JavaScript — no framework, no build step.

| File | Purpose |
| --- | --- |
| `public/index.html` | The whole page. |
| `public/styles.css` | Screen-recorder HUD look: viewfinder, REC red, timelines. Dark by default, light via `prefers-color-scheme`. |
| `public/main.js` | Copy-to-clipboard and the viewfinder timecode. Optional — the page works without it. |
| `public/404.html` | Served by Pages for unknown paths. |
| `public/_headers` | Security and cache headers for Pages. |
| `wrangler.toml` | Pages project config (`pages_build_output_dir = "public"`). |

## Local preview

```sh
cd landing/public
python3 -m http.server 8000   # http://localhost:8000
```

## Deploy (Cloudflare Pages)

**Git integration (recommended).** In the Cloudflare dashboard:
*Workers & Pages → Create → Pages → Connect to Git*, pick
`elvisbrevi/vidcapture`, then:

- Production branch: `main`
- Framework preset: *None*
- Build command: *(empty)*
- Root directory: `landing`
- Build output directory: `public`

Optionally limit rebuilds to this folder under
*Settings → Builds → Build watch paths* → include `landing/*`.

**Or from the CLI:**

```sh
cd landing
npx wrangler pages deploy
```

### Custom domain

In the Pages project: *Custom domains → Set up a custom domain* →
`vidcapture.elvisbrevi.cl`. Since `elvisbrevi.cl` is already on Cloudflare,
the `CNAME` record is created automatically.

## Keeping content in sync

The copy is derived from the repo's [`README.md`](../README.md). When flags,
defaults or the label spec table change, update `public/index.html` to match.
