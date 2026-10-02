# vidcapture landing

Source for <https://vidcapture.elvisbrevi.cl>, served by Cloudflare (a Worker
with static assets only) like the other `*.elvisbrevi.cl` side projects.

Plain HTML, CSS and a few lines of JavaScript — no framework, no build step.

| File | Purpose |
| --- | --- |
| `public/index.html` | The whole page. |
| `public/styles.css` | Screen-recorder HUD look: viewfinder, REC red, timelines. Dark by default, light via `prefers-color-scheme`. |
| `public/main.js` | Copy-to-clipboard and the viewfinder timecode. Optional — the page works without it. |
| `public/404.html` | Served for unknown paths (`not_found_handling = "404-page"`). |
| `public/_headers` | Security and cache headers. |
| `wrangler.toml` | Worker config: serves `public/` as static assets, no Worker script. |

## Local preview

```sh
cd landing/public
python3 -m http.server 8000   # http://localhost:8000
```

## Deploy (Cloudflare Workers)

**Git integration (recommended).** In the Cloudflare dashboard:
*Workers & Pages → Create → Import a repository*, pick
`elvisbrevi/vidcapture`, then:

- Project name: `vidcapture` (must match `name` in `wrangler.toml`)
- Build command: *(empty)*
- Deploy command: `npx wrangler deploy`
- Advanced settings → Path: `landing`

Every push to `main` redeploys. Optionally limit rebuilds to this folder under
*Settings → Build → Build watch paths* → include `landing/*`.

**Or from the CLI:**

```sh
cd landing
npx wrangler deploy
```

### Custom domain

In the Worker: *Settings → Domains & Routes → Add → Custom domain* →
`vidcapture.elvisbrevi.cl`. Since `elvisbrevi.cl` is already on Cloudflare,
the DNS record and certificate are created automatically.

## Keeping content in sync

The copy is derived from the repo's [`README.md`](../README.md). When flags,
defaults or the label spec table change, update `public/index.html` to match.
