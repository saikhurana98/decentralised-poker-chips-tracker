use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

use poker_bank::{
    server::{self, Config},
    store::backup_database,
};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let database = PathBuf::from(env::var("DB_PATH").unwrap_or_else(|_| "./data/poker.db".into()));
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("backup") && args.len() == 1 {
        let directory = env::var_os("BACKUP_DIR").map_or_else(
            || {
                database
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join("backups")
            },
            PathBuf::from,
        );
        let saved = backup_database(&database, &directory)?;
        println!("Backed up {}; kept latest 14 backups", saved.display());
        return Ok(());
    }
    if !args.is_empty() {
        return Err("Usage: poker-bank [backup]".into());
    }
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()?;
    let host = env::var("HOST")
        .unwrap_or_else(|_| "0.0.0.0".into())
        .parse::<IpAddr>()?;
    let trusted_proxies = env::var("TRUSTED_PROXIES")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|ip| ip.trim().parse())
        .collect::<Result<_, _>>()?;
    server::serve(Config {
        address: SocketAddr::new(host, port),
        origin: env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| format!("http://localhost:{port}")),
        database,
        public_directory: PathBuf::from(env::var("PUBLIC_DIR").unwrap_or_else(|_| "public".into())),
        trusted_proxies,
        release_sha: env::var("RELEASE_SHA").unwrap_or_else(|_| "development".into()),
    })
    .await
}
