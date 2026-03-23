use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::model::DetailLevel;

#[derive(Parser, Debug)]
#[command(name = "indxr", version, about = "Fast codebase indexer for AI agents")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Root directory to index
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Output file path (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Output format: markdown, json, or yaml
    #[arg(short, long, default_value = "markdown")]
    pub format: OutputFormat,

    /// Detail level: summary, signatures, or full
    #[arg(short, long, default_value = "signatures")]
    pub detail: DetailLevel,

    /// Maximum directory depth to traverse
    #[arg(long)]
    pub max_depth: Option<usize>,

    /// Skip files larger than N kilobytes
    #[arg(long, default_value = "512")]
    pub max_file_size: u64,

    /// Comma-separated list of languages to include
    #[arg(short, long, value_delimiter = ',')]
    pub languages: Option<Vec<String>>,

    /// Additional glob patterns to exclude
    #[arg(short, long)]
    pub exclude: Option<Vec<String>>,

    /// Do not respect .gitignore
    #[arg(long)]
    pub no_gitignore: bool,

    /// Disable incremental caching
    #[arg(long)]
    pub no_cache: bool,

    /// Cache directory
    #[arg(long, default_value = ".indxr-cache")]
    pub cache_dir: PathBuf,

    /// Suppress progress output
    #[arg(short, long)]
    pub quiet: bool,

    /// Print indexing statistics to stderr
    #[arg(long)]
    pub stats: bool,

    // === New filtering options ===
    /// Filter to a specific subdirectory path
    #[arg(long, value_name = "SUBPATH")]
    pub filter_path: Option<String>,

    /// Search for a specific symbol by name
    #[arg(long)]
    pub symbol: Option<String>,

    /// Filter by declaration kind (e.g., function, struct, class)
    #[arg(long)]
    pub kind: Option<String>,

    /// Only show public declarations
    #[arg(long)]
    pub public_only: bool,

    // === Git-aware diffing ===
    /// Show structural changes since a git ref (branch, tag, or commit)
    #[arg(long, value_name = "REF")]
    pub since: Option<String>,

    // === Token budget ===
    /// Maximum tokens in output (approximate, ~4 chars/token)
    #[arg(long, value_name = "N")]
    pub max_tokens: Option<usize>,

    // === Output control ===
    /// Omit import listings from output
    #[arg(long)]
    pub omit_imports: bool,

    /// Omit directory tree from output
    #[arg(long)]
    pub omit_tree: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start MCP (Model Context Protocol) server for AI agent integration
    Serve {
        /// Root directory to index
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Cache directory
        #[arg(long, default_value = ".indxr-cache")]
        cache_dir: PathBuf,

        /// Skip files larger than N kilobytes
        #[arg(long, default_value = "512")]
        max_file_size: u64,

        /// Maximum directory depth to traverse
        #[arg(long)]
        max_depth: Option<usize>,

        /// Additional glob patterns to exclude
        #[arg(short, long)]
        exclude: Option<Vec<String>>,

        /// Do not respect .gitignore
        #[arg(long)]
        no_gitignore: bool,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Markdown,
    Json,
    Yaml,
}
