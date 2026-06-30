-- Register tree-sitter parser for phox filetype
vim.treesitter.language.register("phox", "phox")

-- Phox LSP configuration
-- Automatically starts the Phox language server for .px files
-- Set PHOX_LSP to override the LSP command
vim.api.nvim_create_autocmd("FileType", {
  pattern = "phox",
  callback = function()
    local lsp_cmd = vim.env.PHOX_LSP
    local cmd
    if lsp_cmd then
      cmd = vim.split(lsp_cmd, " ")
    else
      cmd = { "phox", "lsp" }
    end
    vim.lsp.start({
      name = "phox-lsp",
      cmd = cmd,
      root_dir = vim.fs.dirname(
        vim.fs.find({ "flake.nix", ".git" }, { upward = true })[1]
      ),
    })
  end,
})
