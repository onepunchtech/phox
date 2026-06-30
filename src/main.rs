use clap::Parser;
use phox::{Phox, TermPrinter};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "phox", about = "Phox - a dependently typed expression language")]
enum Cli {
    /// Parse a .px file and print the AST
    Parse {
        /// Path to the .px file
        file: PathBuf,
    },
    /// Lex a .px file and print the token stream
    Lex {
        /// Path to the .px file
        file: PathBuf,
    },
    /// Elaborate and evaluate a .px file, print the normal form and type
    Eval {
        /// Path to the .px file
        file: PathBuf,
    },
    /// Elaborate a .px file, print the elaborated core term and its type
    Elab {
        /// Path to the .px file
        file: PathBuf,
    },
    /// Start the LSP server (stdio)
    #[cfg(feature = "lsp")]
    Lsp,
    /// Format a .px file
    #[cfg(feature = "format")]
    Fmt {
        /// Path to the .px file
        file: PathBuf,
        /// Write formatted output back to file
        #[arg(long)]
        write: bool,
        /// Check if file is already formatted (exit 1 if not)
        #[arg(long)]
        check: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let phox = Phox::new();

    match cli {
        #[cfg(feature = "lsp")]
        Cli::Lsp => {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(phox::lsp::run_server());
            return;
        }
        Cli::Parse { file } => {
            let source = read_file(&file);
            let filename = file.to_string_lossy();
            match phox.parse(&source) {
                Ok(ast) => println!("{ast:#?}"),
                Err(e) => {
                    e.report(&source, &filename);
                    std::process::exit(1);
                }
            }
        }
        Cli::Lex { file } => {
            let source = read_file(&file);
            match phox::lexer::lex(&source) {
                Ok(tokens) => {
                    for (tok, span) in &tokens {
                        println!("{span:?} {tok}");
                    }
                }
                Err(err) => {
                    eprintln!("Lex error: {err}");
                    std::process::exit(1);
                }
            }
        }
        Cli::Eval { file } => {
            let filename = file.to_string_lossy().to_string();
            let result = if file.exists() && file.is_file() {
                phox.eval_file(&file)
            } else {
                let source = read_file(&file);
                phox.eval(&source)
            };
            match result {
                Ok(result) => {
                    println!("{}", TermPrinter(&result.term));
                    print!("  : ");
                    println!("{}", TermPrinter(&result.ty_term));
                }
                Err(e) => {
                    let source = std::fs::read_to_string(&file).unwrap_or_default();
                    e.report(&source, &filename);
                    std::process::exit(1);
                }
            }
        }
        Cli::Elab { file } => {
            let source = read_file(&file);
            let filename = file.to_string_lossy();
            match phox.elaborate(&source) {
                Ok(result) => {
                    println!("{}", TermPrinter(&result.term));
                    println!("  : {}", TermPrinter(&result.ty_term));
                }
                Err(e) => {
                    e.report(&source, &filename);
                    std::process::exit(1);
                }
            }
        }
        #[cfg(feature = "format")]
        Cli::Fmt { file, write, check } => {
            let source = read_file(&file);
            let filename = file.to_string_lossy();
            match phox.format(&source) {
                Ok(formatted) => {
                    if check {
                        if formatted != source {
                            eprintln!("{filename}: not formatted");
                            std::process::exit(1);
                        }
                    } else if write {
                        std::fs::write(&file, &formatted).unwrap_or_else(|e| {
                            eprintln!("Error writing {}: {e}", file.display());
                            std::process::exit(1);
                        });
                    } else {
                        print!("{formatted}");
                    }
                }
                Err(e) => {
                    e.report(&source, &filename);
                    std::process::exit(1);
                }
            }
        }
    }
}

fn read_file(file: &PathBuf) -> String {
    std::fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {e}", file.display());
        std::process::exit(1);
    })
}
