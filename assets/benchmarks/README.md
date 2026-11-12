# Benchmark chart provenance

Source repository: [Proa-Labs/framework-benchmarks](https://github.com/Proa-Labs/framework-benchmarks).
Source revision: `04c702ba2f959ef711551604c3f83a3123d1b1e8`
Measured Proa revision: `bbbc9684335109949257eea8b4c98bb7bc8572ae`.
The benchmark repository and figure generators are Apache-2.0.

| Local filename | Source at the recorded revision | Treatment |
| --- | --- | --- |
| `01-canonical-scoreboard.svg` | `charts/svg/01-canonical-scoreboard.svg` | Exact copy |
| `21-canonical-pages-http.svg` | `charts/svg/08-canonical-pages-http.svg` | Exact copy |
| `22-cold-start.svg` | `charts/svg/09-cold-start.svg` | Exact copy |
| `20-suspense-load.svg` | `charts/figures/suspense.mjs`, using `sveltekit.md` | Matched-host Proa/SvelteKit series only; two-column latency layout |

The first three captions and this README describe a Proa-only November 11,
2026 refresh on Apple M4 Max. Peer rows retain October 21 measurements on the
same machine. The render-only Proa lane includes fresh buffer allocation and
excludes String conversion. Cold-start values include warm filesystem caches
and a shared host with other work active.

The upstream four-engine streaming chart combines November 10 Xeon Marko/React
measurements with Apple M4 Max Proa/SvelteKit measurements despite a same-host
caption. Our derived streaming chart excludes the Xeon rows, adjusts the legend
and latency-panel widths, and labels the actual dates and host. All retained
measurements come unchanged from the upstream Proa/SvelteKit table. Its HTTP
completion boundary delivers data for SvelteKit and resolved HTML for Proa;
[browser completion](https://github.com/Proa-Labs/framework-benchmarks/blob/04c702ba2f959ef711551604c3f83a3123d1b1e8/sveltekit.md#browser-completion)
is reported separately.

The website copies of the render, HTTP, and streaming SVGs under
`proa_marketing/public/images/benchmarks/` match these files byte for byte.
The existing local filenames remain stable so public image URLs keep working.

## Refresh

Check out the source revision above and install its pinned dependencies, then:

```sh
node scripts/sync_benchmark_assets.mjs /path/to/framework-benchmarks
node scripts/sync_benchmark_assets.mjs /path/to/framework-benchmarks --check
```

When selecting a newer source revision, review the README, website chart
captions, and performance guide against its raw captures as well as regenerating
the SVGs. Do not describe retained peer rows as a simultaneous rerun.
