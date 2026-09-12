" Requires vim-lsp and proa-lsp on PATH. Disable any other Rust LSP registration.
augroup proa_lsp
  autocmd!
  autocmd User lsp_setup call lsp#register_server({
        \ 'name': 'proa',
        \ 'cmd': {server_info -> ['proa-lsp', '--stdio']},
        \ 'allowlist': ['rust'],
        \ 'workspace_config': {'rust-analyzer': {}},
        \ })
augroup END
