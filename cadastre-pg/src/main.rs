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

use cli::{Commands, PostgisArgs};

/// Exporter les données cadastrales EDIGEO vers PostGIS ou GeoJSON
#[derive(Parser)]
#[command(name = "cadastre-pg")]
#[command(author, version)]
#[command(about = "Exporter les données cadastrales EDIGEO vers PostGIS (défaut) ou GeoJSON")]
#[command(long_about = "Outil performant pour exporter le cadastre EDIGEO vers PostGIS avec versioning temporel.\n\nPar défaut, exporte vers PostGIS. Utilisez 'to-geojson' pour exporter en GeoJSON.")]
#[command(args_conflicts_with_subcommands = true)]
struct Cli {
    /// Augmenter la verbosité (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Mode silencieux
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Sous-commande (défaut: export vers PostGIS)
    #[command(subcommand)]
    command: Option<Commands>,

    /// Arguments pour l'export PostGIS (commande par défaut)
    #[command(flatten)]
    postgis: Option<PostgisArgs>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Charger .env avant tout
    load_env();

    let cli = Cli::parse();

    // Configurer le logging
    init_logging(cli.verbose, cli.quiet);

    match cli.command {
        Some(Commands::ToGeojson { path, output, srid }) => {
            info!(path = %path.display(), output = %output.display(), srid = ?srid, "Export vers GeoJSON");
            cli::cmd_export(&path, &output, srid).await?;
        }
        None => {
            // Commande par défaut: PostGIS
            let args = cli.postgis.expect("Arguments PostGIS requis (--path et --date)");
            info!(path = %args.path.display(), date = %args.date, "Export vers PostGIS");
            cli::cmd_import(
                &args.path,
                &args.date,
                &args.schema,
                &args.config,
                args.drop_schema,
                args.drop_table,
                args.skip_indexes,
                args.srid,
                args.precision,
                args.dep,
                args.host,
                args.database,
                args.user,
                args.password,
                args.port,
                args.ssl,
                args.jobs,
            )
            .await?;
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
