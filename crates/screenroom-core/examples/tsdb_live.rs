use screenroom_core::{
    MediaKind, SubtitleProvider, SubtitleProviderConfig, SubtitleSearch, TsdbProvider,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("SCREENROOM_TSDB_LIVE").is_none() {
        eprintln!(
            "tsdb_live makes real network requests to The Subtitle DB.\n\
             Run it deliberately: SCREENROOM_TSDB_LIVE=1 cargo run --release \
             -p screenroom-core --example tsdb_live"
        );
        std::process::exit(2);
    }

    let provider = TsdbProvider::new(SubtitleProviderConfig::from_environment());
    let capability = provider.capability();
    println!("provider: {}", capability.provider);
    println!("configured: {}", capability.is_configured());
    println!("detail: {}", capability.detail);
    println!("keyed search: {}", provider.search_is_keyed());
    if !capability.is_configured() {
        return Err("the provider is not configured".into());
    }

    let lookup = SubtitleSearch::movie("Parasite", Some(2019), "bn").with_tmdb(496243);
    let found = provider.search(&lookup)?;
    println!("lookup matches: {}", found.len());
    let Some(chosen) = found.first() else {
        return Err("the anonymous lookup returned nothing".into());
    };
    println!(
        "first match: id={} language={} format={} cues={}",
        chosen.id, chosen.language, chosen.format, chosen.cues
    );

    let searched = provider.search(&SubtitleSearch::movie("Parasite", Some(2019), "en"))?;
    println!("keyed search matches: {}", searched.len());
    if provider.search_is_keyed() && searched.is_empty() {
        return Err("a keyed search returned nothing".into());
    }

    let downloaded = provider.download(chosen.id, &chosen.language, &chosen.format)?;
    let text = String::from_utf8_lossy(&downloaded.bytes);
    if downloaded.bytes.is_empty() {
        return Err("the download was empty".into());
    }
    if downloaded.bytes.contains(&0) {
        return Err("the download was not text".into());
    }
    let cues = text.matches("-->").count();
    println!(
        "downloaded: {} bytes, format {}, cues observed {}, first cue on line {}",
        downloaded.bytes.len(),
        downloaded.format,
        cues,
        text.lines()
            .position(|line| line.contains("-->"))
            .unwrap_or(0)
    );
    if cues == 0 {
        return Err("the download held no subtitle cues".into());
    }

    let episode =
        SubtitleSearch::episode("Game of Thrones", Some(2011), 4, 2, "bn").with_imdb("tt0944947");
    let episodes = provider.search(&episode)?;
    println!("episode matches: {}", episodes.len());
    if episodes.is_empty() {
        return Err("the episode lookup returned nothing".into());
    }
    if episodes
        .iter()
        .any(|candidate| candidate.language_name.is_empty())
    {
        return Err("a match carried no language name".into());
    }
    if MediaKind::Series != episode.kind {
        return Err("the episode request lost its kind".into());
    }

    println!("tsdb_live: ok");
    Ok(())
}
