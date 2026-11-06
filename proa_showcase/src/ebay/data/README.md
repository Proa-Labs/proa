# eBay comparison fixture

The live fixture uses `srp_real_style.css`, `srp_chips_style.css`, and the
renderers in `srp_chrome_real.rs`, `react-ssr/chrome.generated.tsx`, and
`marko-server/page_srp_real.marko`. Keep these generated renderers together.
The superseded handwritten chrome and shell-splicing benchmarks were retired.

The sanitized original capture remains in Git history at
[`de131f79`](https://github.com/Proa-Labs/proa/blob/de131f79/proa_showcase/src/ebay/data/srp_chrome_reference.html).
It is no longer duplicated in the source tree alongside the generated output.
To reproduce the renderers, retrieve that immutable input and run from the repo root:

```bash
gh api -H 'Accept: application/vnd.github.raw+json' \
  'repos/Proa-Labs/proa/contents/proa_showcase/src/ebay/data/srp_chrome_reference.html?ref=de131f79' \
  > /tmp/srp_chrome_reference.html
python3 proa_demo/scripts/gen_srp_chrome.py /tmp/srp_chrome_reference.html
```

The generator sanitizes the input before producing any renderer. Product images
and external styles retain the provenance described in `docs/asset-provenance.md`.
