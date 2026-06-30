# Phox

A dependently typed expression language.

## Features

- Dependent types with bidirectional type checking
- Pattern matching on variants (sum types)
- Records and nested record access
- Isorecursive types (`Mu`)
- Higher-order functions and polymorphism
- Module system with imports
- Language server (LSP)
- Code formatter
- Tree-sitter grammar for syntax highlighting

## Example

```
let
  Maybe : Type -> Type
  Maybe t = < 'Nothing | 'Just t >

  fromMaybe : (a : Type) -> a -> Maybe a -> a
  fromMaybe _ default val = case val of
    'Just x => x
    'Nothing => default
in fromMaybe Integer 0 ('Just 42)
```

More examples in [`examples/`](examples/).

## Building

Requires Rust (2024 edition).

```sh
cargo build
```

Or with Nix:

```sh
nix build .#phox
```

## Usage

```sh
# Evaluate a .px file
phox eval examples/variant.px

# Type-check and show elaborated term
phox elab examples/linked_list.px

# Format a file
phox fmt examples/record.px

# Start the LSP server
phox lsp
```

## Editor Support

A Neovim plugin with tree-sitter highlighting and LSP integration is included in `editor/nvim/`.

With the Nix dev shell, `phox-vim` launches Neovim with everything configured:

```sh
nix develop
phox-vim examples/linked_list.px
```

## License

MIT
