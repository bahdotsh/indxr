use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

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

    // === Complexity hotspots ===
    /// Show complexity hotspots (most complex functions) and exit
    #[arg(long)]
    pub hotspots: bool,

    // === Dependency graph ===
    /// Output dependency graph instead of index (dot, mermaid, or json)
    #[arg(long, value_name = "FORMAT")]
    pub graph: Option<GraphFormat>,

    /// Graph granularity: file-to-file imports or symbol-to-symbol relationships (default: file)
    #[arg(long, requires = "graph", value_name = "LEVEL")]
    pub graph_level: Option<GraphLevel>,

    /// Max edge hops from scoped files in --graph mode (default: unlimited)
    #[arg(long, requires = "graph")]
    pub graph_depth: Option<usize>,
}

/// Options shared between `serve` and `watch` subcommands.
#[derive(Args, Debug, Clone)]
pub struct IndexOpts {
    /// Root directory to index/watch
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Cache directory
    #[arg(long, default_value = ".indxr-cache")]
    pub cache_dir: PathBuf,

    /// Skip files larger than N kilobytes
    #[arg(long, default_value = "512")]
    pub max_file_size: u64,

    /// Maximum directory depth to traverse
    #[arg(long)]
    pub max_depth: Option<usize>,

    /// Additional glob patterns to exclude
    #[arg(short, long)]
    pub exclude: Option<Vec<String>>,

    /// Do not respect .gitignore
    #[arg(long)]
    pub no_gitignore: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start MCP (Model Context Protocol) server for AI agent integration
    Serve {
        #[command(flatten)]
        opts: IndexOpts,

        /// Watch for file changes and auto-reindex
        #[arg(long)]
        watch: bool,

        /// Debounce timeout in milliseconds (requires --watch)
        #[arg(long, default_value = "300", requires = "watch")]
        debounce_ms: u64,
    },

    /// Watch for file changes and keep INDEX.md up to date
    Watch {
        #[command(flatten)]
        opts: IndexOpts,

        /// Output file path (default: INDEX.md in the root directory)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Debounce timeout in milliseconds
        #[arg(long, default_value = "300")]
        debounce_ms: u64,

        /// Suppress progress output
        #[arg(short, long)]
        quiet: bool,
    },

    /// Initialize indxr configuration files for AI agent integration
    Init {
        /// Root directory to initialize
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Set up for Claude Code (.mcp.json, CLAUDE.md, .claude/settings.json)
        #[arg(long)]
        claude: bool,

        /// Set up for Cursor (.cursor/mcp.json, .cursorrules)
        #[arg(long)]
        cursor: bool,

        /// Set up for Windsurf (.windsurf/mcp.json, .windsurfrules)
        #[arg(long)]
        windsurf: bool,

        /// Set up for all supported agents
        #[arg(long, conflicts_with_all = ["claude", "cursor", "windsurf"])]
        all: bool,

        /// Skip generating INDEX.md
        #[arg(long)]
        no_index: bool,

        /// Skip PreToolUse hooks for Claude Code (.claude/settings.json)
        #[arg(long)]
        no_hooks: bool,

        /// Skip RTK hook setup even if rtk is installed
        #[arg(long)]
        no_rtk: bool,

        /// Overwrite existing files without prompting
        #[arg(long)]
        force: bool,

        /// Skip files larger than N kilobytes (passed to indexer)
        #[arg(long, default_value = "512")]
        max_file_size: u64,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum OutputFormat {
    Markdown,
    Json,
    Yaml,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum GraphFormat {
    Dot,
    Mermaid,
    Json,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum GraphLevel {
    File,
    Symbol,
}
