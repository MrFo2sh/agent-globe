mod app;

use clap::Parser;
use std::path::PathBuf;

/// GPU-rendered 3D globe of coding-agent sessions, subagents and their terminal executions.
#[derive(Parser, Debug)]
#[command(name = "agent-globe", version)]
struct Args {
    /// Claude config dir (default: $CLAUDE_CONFIG_DIR or ~/.claude)
    #[arg(long)]
    claude_dir: Option<PathBuf>,
    /// Port for the local HTTP ingest endpoint
    #[arg(long, default_value_t = 4711)]
    port: u16,
    /// Disable the HTTP ingest endpoint
    #[arg(long)]
    no_http: bool,
    /// Open this session id (prefix allowed) on start
    #[arg(long)]
    session: Option<String>,
}

fn main() {
    let args = Args::parse();
    app::run(app::Config {
        claude_dir: args
            .claude_dir
            .unwrap_or_else(agent_globe::data::claude::default_claude_dir),
        http_port: if args.no_http { None } else { Some(args.port) },
        session: args.session,
    });
}
