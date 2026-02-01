//! Point d'entrée CLI pour cadastre-pg

use anyhow::Result;
use clap::Parser;
use tracing::{info, Level};
use tracing_subscriber::{fmt, EnvFilter};

// Charger .env au démarrage
fn load_env() {
    // Chercher .env dans le répertoire courant ou parent
    if dotenvy::dotenv().is_err() {
        // Essayer depuis le répertoire du binaire
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let _ = dotenvy::from_path(dir.join(".env"));
            }
        }
    }
}

mod cli;
mod config;
mod export;
mod versioning;

use cli::Commands;

/// Import EDIGEO cadastral data to PostGIS with temporal versioning
#[derive(Parser)]
#[command(name = "cadastre-pg")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Decrease verbosity (quiet mode)
    #[arg(short, long)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Charger .env avant tout
    load_env();

    let cli = Cli::parse();

    // Configurer le logging
    init_logging(cli.verbose, cli.quiet);

    match cli.command {
        Commands::Import {
            path,
            date,
            schema,
            config,
            drop_schema,
            drop_table,
            skip_indexes,
            srid,
            precision,
            dep,
            host,
            database,
            user,
            password,
            port,
            jobs,
        } => {
            info!(path = %path.display(), date = %date, "Importing EDIGEO data");
            cli::cmd_import(
                &path,
                &date,
                &schema,
                &config,
                drop_schema,
                drop_table,
                skip_indexes,
                srid,
                precision,
                dep,
                host,
                database,
                user,
                password,
                port,
                jobs,
            )
            .await?;
        }
        Commands::Export { path, output, srid } => {
            info!(path = %path.display(), output = %output.display(), srid = ?srid, "Exporting to GeoJSON");
            cli::cmd_export(&path, &output, srid).await?;
        }
    }

    Ok(())
}

fn init_logging(verbose: u8, quiet: bool) {
    let level = match (quiet, verbose) {
        (true, _) => Level::WARN,
        (_, 0) => Level::INFO,
        (_, 1) => Level::DEBUG,
        (_, _) => Level::TRACE,
    };

    let filter = EnvFilter::from_default_env().add_directive(level.into());

    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_file(false)
        .with_line_number(false)
        .init();
}
