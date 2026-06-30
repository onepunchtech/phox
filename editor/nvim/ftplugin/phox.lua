vim.bo.commentstring = "-- %s"
vim.bo.tabstop = 2
vim.bo.shiftwidth = 2
vim.bo.expandtab = true

-- Enable tree-sitter highlighting
vim.treesitter.start()

-- Format on save via LSP
vim.api.nvim_create_autocmd("BufWritePre", {
  buffer = 0,
  callback = function()
    vim.lsp.buf.format({ async = false })
  end,
})
