#[macro_use]
extern crate log;

mod processor;
use processor::ItemsProcessor;

use librespot_core::{
    authentication::Credentials, cache::Cache, config::SessionConfig, session::Session,
};
use librespot_oauth::get_access_token;
use std::{
    env,
    io::{self, BufRead},
    process::exit,
};

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let curr_dir = match env::current_dir() {
        Ok(dir) => dir,
        Err(e) => {
            error!("Failed to get current directory: {}", e);
            exit(1);
        }
    };
    let cache_path = curr_dir.join(".cache");

    let cache = match Cache::new(Some(&cache_path), None, Some(&cache_path), None) {
        Ok(cache) => Some(cache),
        Err(e) => {
            warn!("Cannot create cache: {e}");
            None
        }
    };

    let session_config = SessionConfig::default();

    let credentials = {
        let cached_creds = cache.as_ref().and_then(Cache::credentials);

        if cached_creds.is_some() {
            trace!("Using cached credentials.");
            cached_creds
        } else {
            let access_token = match get_access_token(
                &session_config.client_id,
                &format!("http://127.0.0.1/login"), // no port, to force the use of librespot_oauth::get_authcode_stdin()
                vec!["streaming"],
            ) {
                Ok(token) => token.access_token,
                Err(e) => {
                    error!("Failed to get Spotify access token: {e}");
                    exit(1);
                }
            };
            Some(Credentials::with_access_token(access_token))
        }
    };

    let session = Session::new(session_config, cache);

    if let Err(e) = session
        .connect(credentials.clone().unwrap_or_default(), true)
        .await
    {
        error!("Error connecting: {}", e);
        exit(1);
    }

    info!("Connected!");

    let mut processor = ItemsProcessor::new(session, curr_dir);

    for line in io::stdin().lock().lines() {
        match line {
            Ok(line) => {
                let line = line.trim();
                if line == "done" {
                    break;
                }
                if let Err(e) = processor.load_item(line).await {
                    error!("Failed to load item: {e}");
                }
            }
            Err(e) => error!("ERROR: {e}"),
        }
    }

    if let Err(e) = processor.process_items().await {
        error!("Failed to process items: {e}");
    }
}
