# Proa contributor documentation

This directory contains maintained contributor guides and repository policies.
User-facing website documentation is authored in
[`proa_marketing/src/docs/`](../proa_marketing/src/docs/index.mdx). The website
build does not publish this directory.

## Start here

- [`agent/`](agent/index.md): current component, template, and RSJS authoring.
- [`agent/api-reference/render-traits.md`](agent/api-reference/render-traits.md):
  synchronous, asynchronous, and `Send` rendering.
- [`agent/rsjs-islands.md`](agent/rsjs-islands.md): analyzed components and
  browser interactivity.
- [`../editors/`](../editors/README.md): VS Code and LSP editor support,
  including Neovim setup.
- [`../proa_stream/`](../proa_stream/README.md): streaming rendering and
  framework integration.
- [`migrations/0.3-standalone.md`](migrations/0.3-standalone.md): API changes
  for the standalone 0.3 release.

Start with the root [`README.md`](../README.md) for a project overview and
[`CONTRIBUTING.md`](../CONTRIBUTING.md) for development and validation commands.

## Repository policies

- [`open-source-boundary.md`](open-source-boundary.md): standalone source
  requirements and the public/private documentation boundary.
- [`registry.md`](registry.md): package configuration and publication checks.
- [`asset-provenance.md`](asset-provenance.md): retained assets and provenance.
- [`../SECURITY.md`](../SECURITY.md): reporting vulnerabilities.
- [`../proa_bench/BENCHMARKS.md`](../proa_bench/BENCHMARKS.md): reproducible
  internal regression benchmarks.

## Internal knowledge base

Keep working RFCs, roadmaps, implementation plans, operational runbooks, audit
notes, and superseded guides in private storage. For local work, use the
Git-ignored `.local/knowledge-base/` directory. It is also excluded from Docker
build contexts. Local notes need a separate private backup if they must be
retained or shared.

Do not link public documentation to private notes or include them in build
inputs. Before publishing material from the knowledge base, review it for
confidential information, verify it against current behavior, and rewrite it
as a focused contributor or user guide. Private working records are not part
of the supported API documentation.
