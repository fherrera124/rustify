use librespot_core::{
    audio_key::AudioKeyError,
    spotify_id::{SpotifyId, SpotifyItemType},
    Error, Session,
};
use librespot_metadata::{
    audio::{AudioFiles, UniqueFields},
    Album, Metadata, Playlist, Show,
};
use log::{error, warn};
use regex::Regex;
use sanitize_filename::sanitize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    mem,
    path::PathBuf,
    process::{Command, Stdio},
};

use tokio::time::{sleep, Duration};

mod loader;
use loader::TrackLoader;

#[derive(Hash, Eq, PartialEq, Debug)]
struct GroupPath(String);

pub struct ItemsProcessor {
    session: Session,
    track_loader: TrackLoader,
    base_path: PathBuf,
    grouped_ids: HashMap<GroupPath, HashSet<SpotifyId>>,
    penalty_delay: Duration,
    re: Regex,
}

impl ItemsProcessor {
    pub const DELAY_BETWEEN_ITEMS: u64 = 10;
    pub const MAX_PENALTY_DELAY: u64 = 300;

    pub fn new(session: Session, base_path: PathBuf) -> Self {
        let track_loader = TrackLoader::new(session.clone());
        Self {
            session,
            track_loader,
            base_path,
            grouped_ids: HashMap::new(),
            penalty_delay: Duration::from_secs(0),
            re: Regex::new(r"(playlist|track|album|episode|show)[/:]([a-zA-Z0-9]+)").unwrap(),
        }
    }

    pub async fn load_item(&mut self, line: &str) -> Result<(), Error> {
        let spotify_match = match self.re.captures(line) {
            Some(x) => x,
            None => return Ok(()),
        };

        let item_type_str = spotify_match.get(1).unwrap().as_str();
        let mut spotify_id = SpotifyId::from_base62(spotify_match.get(2).unwrap().as_str())?;
        spotify_id.item_type = SpotifyItemType::from(item_type_str);

        match spotify_id.item_type {
            SpotifyItemType::Playlist => {
                let playlist = Playlist::get(&self.session, &spotify_id).await?;
                let sanitized_name = sanitize(playlist.name()).trim().to_string();
                let path = format!("playlists/{}", sanitized_name);
                self.grouped_ids
                    .entry(GroupPath(path))
                    .or_insert_with(HashSet::new)
                    .extend(playlist.tracks());
            }
            SpotifyItemType::Album => {
                let album = Album::get(&self.session, &spotify_id).await?;
                let sanitized_name = sanitize(&album.name).trim().to_string();
                let path = format!("albums/{}", sanitized_name);
                self.grouped_ids
                    .entry(GroupPath(path))
                    .or_insert_with(HashSet::new)
                    .extend(album.tracks());
            }
            SpotifyItemType::Track => {
                self.grouped_ids
                    .entry(GroupPath("tracks".to_string()))
                    .or_insert_with(HashSet::new)
                    .insert(spotify_id);
            }
            SpotifyItemType::Episode => {
                self.grouped_ids
                    .entry(GroupPath("episodes".to_string()))
                    .or_insert_with(HashSet::new)
                    .insert(spotify_id);
            }
            SpotifyItemType::Show => {
                let show = Show::get(&self.session, &spotify_id).await?;
                let sanitized_name = sanitize(&show.name).trim().to_string();
                let path = format!("shows/{}", sanitized_name);
                self.grouped_ids
                    .entry(GroupPath(path))
                    .or_insert_with(HashSet::new)
                    .extend(show.episodes.0);
            }
            _ => warn!("Unknown/unsupported item type: {}", item_type_str),
        }
        Ok(())
    }

    pub async fn process_items(&mut self) -> Result<(), Error> {
        if self.grouped_ids.is_empty() {
            warn!("No items to process.");
            return Ok(());
        }
        let grouped_ids = mem::take(&mut self.grouped_ids);
        for (group_path, spotify_ids) in grouped_ids {
            if spotify_ids.is_empty() {
                continue;
            }
            let dir_path = self.base_path.join(group_path.0);
            fs::create_dir_all(&dir_path)?;
            for (index, spotify_id) in spotify_ids.iter().enumerate() {
                self.process_single_item(spotify_id, &dir_path).await?;
                if index != spotify_ids.len() - 1 {
                    sleep(Duration::from_secs(ItemsProcessor::DELAY_BETWEEN_ITEMS)).await;
                }
            }
        }
        Ok(())
    }

    async fn process_single_item(
        &mut self,
        spotify_id: &SpotifyId,
        dir_path: &PathBuf,
    ) -> Result<(), Error> {
        loop {
            match self.save_audio_item(spotify_id, dir_path).await {
                Ok(_) => {
                    self.penalty_delay = Duration::from_secs(0);
                    break;
                }
                Err(e) => {
                    if let Some(AudioKeyError::AesKey) = e.error.downcast_ref::<AudioKeyError>() {
                        self.penalty_delay += Duration::from_secs(60);
                        if self.penalty_delay
                            > Duration::from_secs(ItemsProcessor::MAX_PENALTY_DELAY)
                        {
                            return Err(Error::internal(
                                "Error: We cannot delay anymore..., exiting.",
                            ));
                        }
                        warn!(
                            "Warn: Audio key response error. Wait '{}' seconds and retrying...",
                            self.penalty_delay.as_secs()
                        );
                        sleep(self.penalty_delay).await;
                    } else {
                        error!("Error: {:?}", e);
                        // TODO: add to list of failed items
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    async fn save_audio_item(
        &self,
        spotify_id: &SpotifyId,
        dir_path: &PathBuf,
    ) -> Result<(), Error> {
        let track_data = self.track_loader.load_track(*spotify_id).await?;
        let (audio_item, audio_buffer, audio_format) = (
            track_data.audio_item,
            track_data.audio_buffer,
            track_data.audio_format,
        );

        let (origins, group_name) = match &audio_item.unique_fields {
            UniqueFields::Track { artists, album, .. } => (
                artists
                    .0
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<&str>>(),
                album.to_string(),
            ),
            UniqueFields::Episode { show_name, .. } => (Vec::new(), show_name.to_string()),
        };

        let cover = audio_item
            .covers
            .first()
            .ok_or_else(|| Error::not_found("No covers available for this audio item"))?;

        let track_id = audio_item.track_id.to_base62()?;
        let fname = sanitize(format!("{} - {}", audio_item.name, origins.join(", ")))
            .trim()
            .to_string();

        let extension = match audio_format {
            f if AudioFiles::is_ogg_vorbis(f) => "ogg",
            f if AudioFiles::is_mp3(f) => "mp3",
            f if AudioFiles::is_flac(f) => "flac",
            _ => return Err(Error::internal("Unsupported audio format")),
        };

        let full_path = dir_path.join(format!("{}.{}", &fname, extension));
        if full_path.exists() {
            warn!(
                "File '{}' already exists. Skipping",
                full_path.to_str().unwrap()
            );
            return Ok(());
        }
        if let Err(e) = Self::run_helper_script(
            extension,
            &track_id,
            &cover.url,
            full_path.to_str().unwrap(),
            &audio_item.name,
            &group_name,
            origins,
            &audio_buffer,
        ) {
            warn!(
                "Error running helper script: {:?}. Saving file without metadata",
                e
            );
            let mut file = fs::File::create(&full_path)?;
            file.write_all(&audio_buffer)?;
        }
        Ok(())
    }

    fn run_helper_script(
        extension: &str,
        track_id: &str,
        cover_url: &str,
        full_path_str: &str,
        track_title: &str,
        group_name: &str,
        origins: Vec<&str>,
        audio_buffer: &[u8],
    ) -> Result<(), Error> {
        if extension == "ogg" {
            let mut cmd = Command::new("tag_ogg.sh");
            cmd.arg(track_id)
                .arg(track_title)
                .arg(group_name)
                .arg(full_path_str)
                .arg(cover_url)
                .args(origins)
                .stdin(Stdio::piped());

            let mut child = cmd.spawn()?;
            let pipe = child
                .stdin
                .as_mut()
                .ok_or_else(|| Error::internal("Failed to open helper script"))?;
            pipe.write_all(audio_buffer)?;
            let status = child.wait()?;
            if !status.success() {
                return Err(Error::internal("Helper script returned an error"));
            }
            return Ok(());
        }
        Err(Error::internal(format!(
            "No script for extension {extension}"
        )))
    }
}
