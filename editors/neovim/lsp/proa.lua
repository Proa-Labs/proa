-- Neovim 0.11+. Use this config OR wrap your existing rust_analyzer config.
return {
  cmd = { 'proa-lsp', '--stdio' },
  filetypes = { 'rust' },
  root_markers = { 'Cargo.toml', 'rust-project.json', '.git' },
  settings = { ['rust-analyzer'] = {} },
}
