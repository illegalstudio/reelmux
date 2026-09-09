use std::{collections::BTreeMap, env, fmt, str::FromStr, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use crate::language;

const APPLE_OPTIONS: &[(&str, &str)] = &[
    ("utsk", "0"),
    ("caller", "wta"),
    ("v", "58"),
    ("pfm", "appletv"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    AppleTv,
    Tmdb,
    Tvdb,
    #[serde(rename = "itunes")]
    ITunes,
}

impl Provider {
    pub const ALL: [Self; 4] = [Self::AppleTv, Self::Tmdb, Self::Tvdb, Self::ITunes];

    pub fn label(self) -> &'static str {
        match self {
            Self::AppleTv => "Apple TV",
            Self::Tmdb => "TheMovieDB",
            Self::Tvdb => "TheTVDB",
            Self::ITunes => "iTunes Store",
        }
    }

    pub fn attribution(self) -> &'static str {
        match self {
            Self::AppleTv => "Metadati e immagini: Apple TV",
            Self::Tmdb => {
                "This product uses the TMDB API but is not endorsed or certified by TMDB."
            }
            Self::Tvdb => {
                "Metadata provided by TheTVDB. Please consider adding missing information or subscribing."
            }
            Self::ITunes => "Metadati e immagini: iTunes Store",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

impl FromStr for Provider {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "apple" | "apple-tv" | "appletv" => Ok(Self::AppleTv),
            "tmdb" | "themoviedb" => Ok(Self::Tmdb),
            "tvdb" | "thetvdb" => Ok(Self::Tvdb),
            "itunes" | "itunes-store" => Ok(Self::ITunes),
            _ => bail!("Provider non valido: {value}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Movie,
    TvShow,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Movie => "Film",
            Self::TvShow => "Serie TV",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchQuery {
    pub provider: Provider,
    pub kind: MediaKind,
    pub term: String,
    pub language: String,
    pub country: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

impl SearchQuery {
    pub fn italian(provider: Provider, kind: MediaKind, term: impl Into<String>) -> Self {
        Self {
            provider,
            kind,
            term: term.into(),
            language: "it-IT".into(),
            country: "IT".into(),
            season: None,
            episode: None,
        }
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            !self.term.trim().is_empty(),
            "Inserisci un titolo da cercare"
        );
        ensure!(
            self.language.len() >= 2 && self.language.is_ascii(),
            "Codice lingua non valido"
        );
        ensure!(
            self.country.len() == 2 && self.country.is_ascii(),
            "Il paese deve essere un codice ISO di due lettere"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchHit {
    pub provider: Provider,
    pub kind: MediaKind,
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub overview: String,
    pub thumbnail_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetadataResult {
    pub provider: Provider,
    pub fields: BTreeMap<String, String>,
    pub artwork_url: Option<String>,
    pub source_url: Option<String>,
    pub attribution: String,
}

#[derive(Clone, Debug)]
pub struct Artwork {
    pub bytes: Vec<u8>,
    pub media_type: String,
    pub source_url: String,
    pub provider: Provider,
}

#[derive(Clone, Debug, Default)]
pub struct Credentials {
    pub tmdb_token: Option<String>,
    pub tmdb_api_key: Option<String>,
    pub tvdb_api_key: Option<String>,
    pub tvdb_pin: Option<String>,
}

impl Credentials {
    pub fn from_env() -> Self {
        fn value(name: &str) -> Option<String> {
            env::var(name).ok().filter(|value| !value.trim().is_empty())
        }
        Self {
            tmdb_token: value("TMDB_API_TOKEN"),
            tmdb_api_key: value("TMDB_API_KEY"),
            tvdb_api_key: value("TVDB_API_KEY"),
            tvdb_pin: value("TVDB_PIN"),
        }
    }
}

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    credentials: Credentials,
    apple_base: String,
    itunes_base: String,
    tmdb_base: String,
    tvdb_base: String,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(Credentials::from_env())
    }
}

impl Client {
    pub fn new(credentials: Credentials) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(25)))
            .user_agent(concat!("Subler-Linux/", env!("CARGO_PKG_VERSION")))
            .build();
        Self {
            agent: config.into(),
            credentials,
            apple_base: "https://uts-api.itunes.apple.com/uts/v2/".into(),
            itunes_base: "https://itunes.apple.com/".into(),
            tmdb_base: "https://api.themoviedb.org/3/".into(),
            tvdb_base: "https://api4.thetvdb.com/v4/".into(),
        }
    }

    pub fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        query.validate()?;
        match query.provider {
            Provider::AppleTv => self.search_apple(query),
            Provider::Tmdb => self.search_tmdb(query),
            Provider::Tvdb => self.search_tvdb(query),
            Provider::ITunes => self.search_itunes(query),
        }
    }

    pub fn resolve(&self, hit: &SearchHit, query: &SearchQuery) -> Result<MetadataResult> {
        ensure!(
            hit.provider == query.provider && hit.kind == query.kind,
            "Il risultato non appartiene alla ricerca corrente"
        );
        match hit.provider {
            Provider::AppleTv => self.resolve_apple(hit, query),
            Provider::Tmdb => self.resolve_tmdb(hit, query),
            Provider::Tvdb => self.resolve_tvdb(hit, query),
            Provider::ITunes => self.resolve_itunes(hit, query),
        }
    }

    pub fn download_artwork(&self, result: &MetadataResult) -> Result<Option<Artwork>> {
        let Some(source_url) = result.artwork_url.as_deref() else {
            return Ok(None);
        };
        let url = Url::parse(source_url).context("URL della locandina non valido")?;
        ensure!(url.scheme() == "https", "La locandina deve usare HTTPS");
        let mut response = self.agent.get(url.as_str()).call().with_context(|| {
            format!(
                "Download della locandina da {} non riuscito",
                result.provider
            )
        })?;
        let media_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        ensure!(
            matches!(media_type.as_str(), "image/jpeg" | "image/png"),
            "Formato della locandina non supportato: {media_type}"
        );
        let bytes = response
            .body_mut()
            .with_config()
            .limit(15 * 1024 * 1024)
            .read_to_vec()
            .context("Lettura della locandina non riuscita")?;
        let valid = match media_type.as_str() {
            "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
            "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
            _ => false,
        };
        ensure!(valid, "Il server non ha restituito un’immagine valida");
        Ok(Some(Artwork {
            bytes,
            media_type,
            source_url: source_url.into(),
            provider: result.provider,
        }))
    }

    fn get_json(&self, url: Url, bearer: Option<&str>, provider: Provider) -> Result<Value> {
        let mut request = self
            .agent
            .get(url.as_str())
            .header("Accept", "application/json");
        if let Some(token) = bearer {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        request
            .call()
            .with_context(|| format!("Richiesta a {provider} non riuscita"))?
            .body_mut()
            .read_json()
            .with_context(|| format!("Risposta JSON di {provider} non valida"))
    }

    fn get_json_optional(&self, url: Url, bearer: Option<&str>) -> Option<Value> {
        let mut request = self
            .agent
            .get(url.as_str())
            .header("Accept", "application/json");
        if let Some(token) = bearer {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        request.call().ok()?.body_mut().read_json().ok()
    }

    fn apple_url(&self, path: &str, query: &SearchQuery) -> Result<Url> {
        let mut url = Url::parse(&self.apple_base)?.join(path)?;
        let storefront = apple_storefront(&query.country)
            .context("Paese non supportato da Apple TV. Usa IT, US, GB, DE, FR, ES, CA, AU o JP")?;
        url.query_pairs_mut()
            .append_pair("sf", storefront)
            .append_pair("locale", &query.language);
        for (key, value) in APPLE_OPTIONS {
            url.query_pairs_mut().append_pair(key, value);
        }
        Ok(url)
    }

    fn search_apple(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let mut url = self.apple_url("search/incremental", query)?;
        url.query_pairs_mut().append_pair("q", query.term.trim());
        let value = self.get_json(url, None, Provider::AppleTv)?;
        let wanted = if query.kind == MediaKind::Movie {
            "Movie"
        } else {
            "Show"
        };
        let items = value
            .pointer("/data/canvas/shelves")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|shelf| shelf.get("items").and_then(Value::as_array))
            .flatten()
            .filter(|item| string(item, "type") == wanted)
            .filter_map(|item| {
                Some(SearchHit {
                    provider: Provider::AppleTv,
                    kind: query.kind,
                    id: string(item, "id").to_owned(),
                    title: nonempty(string(item, "title"))?.to_owned(),
                    subtitle: apple_date(item.get("releaseDate")),
                    overview: string(item, "description").to_owned(),
                    thumbnail_url: apple_image(item, "coverArt", 300, 450),
                })
            })
            .take(40)
            .collect();
        Ok(items)
    }

    fn resolve_apple(&self, hit: &SearchHit, query: &SearchQuery) -> Result<MetadataResult> {
        let path = format!("view/product/{}", hit.id);
        let value = self.get_json(self.apple_url(&path, query)?, None, Provider::AppleTv)?;
        let content = value.pointer("/data/content").unwrap_or(&Value::Null);
        let roles = value
            .pointer("/data/roles")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut fields = base_fields(
            string(content, "title").or_if_empty(&hit.title),
            string(content, "description").or_if_empty(&hit.overview),
            &apple_date(content.get("releaseDate")),
        );
        insert(&mut fields, "genre", joined_names(content.get("genres")));
        insert(&mut fields, "studio", string(content, "studio"));
        insert(
            &mut fields,
            "content_rating",
            content
                .pointer("/rating/displayName")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        insert(&mut fields, "cast", apple_roles(roles, &["Actor", "Voice"]));
        insert(&mut fields, "director", apple_roles(roles, &["Director"]));
        insert(&mut fields, "producers", apple_roles(roles, &["Producer"]));
        insert(
            &mut fields,
            "screenwriters",
            apple_roles(roles, &["Writer"]),
        );
        insert(&mut fields, "composer", apple_roles(roles, &["Music"]));
        if query.kind == MediaKind::TvShow {
            fields.insert("show".into(), hit.title.clone());
            if let (Some(season), Some(episode)) = (query.season, query.episode)
                && let Some(item) = self.apple_episode(&hit.id, season, episode, query)?
            {
                insert(&mut fields, "title", string(&item, "title"));
                insert(&mut fields, "description", string(&item, "description"));
                insert(&mut fields, "date", apple_date(item.get("releaseDate")));
                fields.insert("season_number".into(), season.to_string());
                fields.insert("episode_sort".into(), episode.to_string());
            }
        }
        fields.insert("provider".into(), Provider::AppleTv.label().into());
        fields.insert("provider_id".into(), hit.id.clone());
        let source_url = nonempty(string(content, "url")).map(str::to_owned);
        Ok(MetadataResult {
            provider: Provider::AppleTv,
            fields,
            artwork_url: apple_image(content, "coverArt", 1200, 1800)
                .or_else(|| hit.thumbnail_url.clone()),
            source_url,
            attribution: Provider::AppleTv.attribution().into(),
        })
    }

    fn apple_episode(
        &self,
        show_id: &str,
        season: u32,
        episode: u32,
        query: &SearchQuery,
    ) -> Result<Option<Value>> {
        let path = format!("view/show/{show_id}/episodes");
        let mut url = self.apple_url(&path, query)?;
        url.query_pairs_mut()
            .append_pair("count", "1000")
            .append_pair("skip", "0");
        let value = self.get_json(url, None, Provider::AppleTv)?;
        Ok(value
            .pointer("/data/episodes")
            .and_then(Value::as_array)
            .and_then(|episodes| {
                episodes.iter().find(|item| {
                    integer(item, "seasonNumber") == Some(season as i64)
                        && integer(item, "episodeNumber") == Some(episode as i64)
                })
            })
            .cloned())
    }

    fn tmdb_auth(&self, mut url: Url) -> Result<(Url, Option<&str>)> {
        if let Some(token) = self.credentials.tmdb_token.as_deref() {
            Ok((url, Some(token)))
        } else if let Some(key) = self.credentials.tmdb_api_key.as_deref() {
            url.query_pairs_mut().append_pair("api_key", key);
            Ok((url, None))
        } else {
            bail!("TheMovieDB richiede TMDB_API_TOKEN oppure TMDB_API_KEY")
        }
    }

    fn search_tmdb(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let path = if query.kind == MediaKind::Movie {
            "search/movie"
        } else {
            "search/tv"
        };
        let mut url = Url::parse(&self.tmdb_base)?.join(path)?;
        url.query_pairs_mut()
            .append_pair("query", query.term.trim())
            .append_pair("language", &query.language)
            .append_pair("include_adult", "false");
        let (url, token) = self.tmdb_auth(url)?;
        let value = self.get_json(url, token, Provider::Tmdb)?;
        let hits = value["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let title_key = if query.kind == MediaKind::Movie {
                    "title"
                } else {
                    "name"
                };
                let date_key = if query.kind == MediaKind::Movie {
                    "release_date"
                } else {
                    "first_air_date"
                };
                Some(SearchHit {
                    provider: Provider::Tmdb,
                    kind: query.kind,
                    id: item.get("id")?.as_i64()?.to_string(),
                    title: nonempty(string(item, title_key))?.to_owned(),
                    subtitle: string(item, date_key).to_owned(),
                    overview: string(item, "overview").to_owned(),
                    thumbnail_url: tmdb_image(item.get("poster_path"), "w342"),
                })
            })
            .collect();
        Ok(hits)
    }

    fn resolve_tmdb(&self, hit: &SearchHit, query: &SearchQuery) -> Result<MetadataResult> {
        let resource = if hit.kind == MediaKind::Movie {
            "movie"
        } else {
            "tv"
        };
        let mut url = Url::parse(&self.tmdb_base)?.join(&format!("{resource}/{}", hit.id))?;
        url.query_pairs_mut()
            .append_pair("language", &query.language)
            .append_pair(
                "append_to_response",
                "credits,content_ratings,release_dates,images,external_ids",
            );
        let (url, token) = self.tmdb_auth(url)?;
        let value = self.get_json(url, token, Provider::Tmdb)?;
        let title_key = if hit.kind == MediaKind::Movie {
            "title"
        } else {
            "name"
        };
        let date_key = if hit.kind == MediaKind::Movie {
            "release_date"
        } else {
            "first_air_date"
        };
        let mut fields = base_fields(
            string(&value, title_key).or_if_empty(&hit.title),
            string(&value, "overview").or_if_empty(&hit.overview),
            string(&value, date_key),
        );
        insert(&mut fields, "genre", joined_names(value.get("genres")));
        insert(
            &mut fields,
            "studio",
            joined_names(value.get("production_companies")),
        );
        insert(&mut fields, "network", joined_names(value.get("networks")));
        insert(&mut fields, "cast", tmdb_cast(&value));
        insert(&mut fields, "director", tmdb_crew(&value, "Director"));
        insert(&mut fields, "producers", tmdb_crew(&value, "Producer"));
        insert(
            &mut fields,
            "screenwriters",
            tmdb_department(&value, "Writing"),
        );
        insert(
            &mut fields,
            "composer",
            tmdb_crew(&value, "Original Music Composer"),
        );
        let mut artwork_url =
            tmdb_image(value.get("poster_path"), "original").or_else(|| hit.thumbnail_url.clone());
        if hit.kind == MediaKind::TvShow {
            fields.insert(
                "show".into(),
                string(&value, "name").or_if_empty(&hit.title).into(),
            );
            if let (Some(season), Some(episode)) = (query.season, query.episode) {
                let mut episode_url = Url::parse(&self.tmdb_base)?
                    .join(&format!("tv/{}/season/{season}/episode/{episode}", hit.id))?;
                episode_url
                    .query_pairs_mut()
                    .append_pair("language", &query.language)
                    .append_pair("append_to_response", "credits,images,external_ids");
                let (episode_url, token) = self.tmdb_auth(episode_url)?;
                let item = self.get_json(episode_url, token, Provider::Tmdb)?;
                insert(&mut fields, "title", string(&item, "name"));
                insert(&mut fields, "description", string(&item, "overview"));
                insert(&mut fields, "date", string(&item, "air_date"));
                fields.insert("season_number".into(), season.to_string());
                fields.insert("episode_sort".into(), episode.to_string());
                insert(&mut fields, "director", tmdb_crew(&item, "Director"));
                insert(
                    &mut fields,
                    "screenwriters",
                    tmdb_department(&item, "Writing"),
                );
                if let Some(url) = tmdb_image(item.get("still_path"), "original") {
                    artwork_url = Some(url);
                }
            }
        }
        fields.insert("provider".into(), Provider::Tmdb.label().into());
        fields.insert("provider_id".into(), hit.id.clone());
        Ok(MetadataResult {
            provider: Provider::Tmdb,
            fields,
            artwork_url,
            source_url: Some(format!("https://www.themoviedb.org/{resource}/{}", hit.id)),
            attribution: Provider::Tmdb.attribution().into(),
        })
    }

    fn tvdb_token(&self) -> Result<String> {
        let key = self
            .credentials
            .tvdb_api_key
            .as_deref()
            .context("TheTVDB richiede TVDB_API_KEY")?;
        let url = Url::parse(&self.tvdb_base)?.join("login")?;
        let mut body = json!({ "apikey": key });
        if let Some(pin) = self.credentials.tvdb_pin.as_deref() {
            body["pin"] = Value::String(pin.into());
        }
        let value: Value = self
            .agent
            .post(url.as_str())
            .header("Accept", "application/json")
            .send_json(&body)
            .context("Accesso a TheTVDB non riuscito")?
            .body_mut()
            .read_json()
            .context("Risposta di accesso TheTVDB non valida")?;
        value
            .pointer("/data/token")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("TheTVDB non ha restituito un token")
    }

    fn search_tvdb(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let token = self.tvdb_token()?;
        let mut url = Url::parse(&self.tvdb_base)?.join("search")?;
        url.query_pairs_mut()
            .append_pair("query", query.term.trim())
            .append_pair(
                "type",
                if query.kind == MediaKind::Movie {
                    "movie"
                } else {
                    "series"
                },
            );
        let value = self.get_json(url, Some(&token), Provider::Tvdb)?;
        let hits = value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                Some(SearchHit {
                    provider: Provider::Tvdb,
                    kind: query.kind,
                    id: nonempty(string(item, "tvdb_id").or_if_empty(string(item, "id")))?
                        .to_owned(),
                    title: nonempty(
                        string(item, "name_translated")
                            .or_if_empty(string(item, "name"))
                            .or_if_empty(string(item, "title")),
                    )?
                    .to_owned(),
                    subtitle: string(item, "year").to_owned(),
                    overview: string(item, "overview").to_owned(),
                    thumbnail_url: absolute_image(
                        item.get("thumbnail").or_else(|| item.get("image_url")),
                    ),
                })
            })
            .collect();
        Ok(hits)
    }

    fn resolve_tvdb(&self, hit: &SearchHit, query: &SearchQuery) -> Result<MetadataResult> {
        let token = self.tvdb_token()?;
        let resource = if hit.kind == MediaKind::Movie {
            "movies"
        } else {
            "series"
        };
        let url = Url::parse(&self.tvdb_base)?.join(&format!("{resource}/{}/extended", hit.id))?;
        let value = self.get_json(url, Some(&token), Provider::Tvdb)?;
        let data = &value["data"];
        let translation = tvdb_language(&query.language)
            .and_then(|language| {
                Url::parse(&self.tvdb_base)
                    .ok()?
                    .join(&format!("{resource}/{}/translations/{language}", hit.id))
                    .ok()
            })
            .and_then(|url| self.get_json_optional(url, Some(&token)))
            .unwrap_or(Value::Null);
        let translated = &translation["data"];
        let mut fields = base_fields(
            string(translated, "name")
                .or_if_empty(string(data, "name"))
                .or_if_empty(&hit.title),
            string(translated, "overview")
                .or_if_empty(string(data, "overview"))
                .or_if_empty(&hit.overview),
            string(data, "firstAired").or_if_empty(string(data, "year")),
        );
        insert(&mut fields, "genre", joined_names(data.get("genres")));
        insert(&mut fields, "studio", joined_names(data.get("studios")));
        insert(&mut fields, "cast", tvdb_people(data, &[3, 4]));
        insert(&mut fields, "director", tvdb_people(data, &[1]));
        let mut artwork_url = absolute_image(data.get("image"))
            .or_else(|| tvdb_poster(data))
            .or_else(|| hit.thumbnail_url.clone());
        if hit.kind == MediaKind::TvShow {
            fields.insert("show".into(), fields["title"].clone());
            if let (Some(season), Some(episode)) = (query.season, query.episode) {
                let mut episodes_url = Url::parse(&self.tvdb_base)?
                    .join(&format!("series/{}/episodes/default", hit.id))?;
                episodes_url
                    .query_pairs_mut()
                    .append_pair("season", &season.to_string())
                    .append_pair("episodeNumber", &episode.to_string());
                let episode_list = self.get_json(episodes_url, Some(&token), Provider::Tvdb)?;
                let item = episode_list
                    .pointer("/data/episodes/0")
                    .context("Episodio non trovato su TheTVDB")?;
                let episode_translation = integer(item, "id")
                    .and_then(|id| {
                        let language = tvdb_language(&query.language)?;
                        Url::parse(&self.tvdb_base)
                            .ok()?
                            .join(&format!("episodes/{id}/translations/{language}"))
                            .ok()
                    })
                    .and_then(|url| self.get_json_optional(url, Some(&token)))
                    .unwrap_or(Value::Null);
                let translated_episode = &episode_translation["data"];
                insert(
                    &mut fields,
                    "title",
                    string(translated_episode, "name").or_if_empty(string(item, "name")),
                );
                insert(
                    &mut fields,
                    "description",
                    string(translated_episode, "overview").or_if_empty(string(item, "overview")),
                );
                insert(&mut fields, "date", string(item, "aired"));
                fields.insert("season_number".into(), season.to_string());
                fields.insert("episode_sort".into(), episode.to_string());
                if let Some(url) = absolute_image(item.get("image")) {
                    artwork_url = Some(url);
                }
            }
        }
        fields.insert("provider".into(), Provider::Tvdb.label().into());
        fields.insert("provider_id".into(), hit.id.clone());
        Ok(MetadataResult {
            provider: Provider::Tvdb,
            fields,
            artwork_url,
            source_url: Some(if hit.kind == MediaKind::Movie {
                format!("https://thetvdb.com/movies/{}", hit.id)
            } else {
                format!("https://thetvdb.com/series/{}", hit.id)
            }),
            attribution: Provider::Tvdb.attribution().into(),
        })
    }

    fn search_itunes(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let mut url = Url::parse(&self.itunes_base)?.join("search")?;
        url.query_pairs_mut()
            .append_pair("term", query.term.trim())
            .append_pair("country", &query.country.to_ascii_uppercase())
            .append_pair(
                "lang",
                &query.language.replace('-', "_").to_ascii_lowercase(),
            )
            .append_pair("limit", "200");
        if query.kind == MediaKind::Movie {
            url.query_pairs_mut().append_pair("entity", "movie");
        } else {
            url.query_pairs_mut()
                .append_pair("media", "tvShow")
                .append_pair("entity", "tvEpisode");
        }
        let value = self.get_json(url, None, Provider::ITunes)?;
        let hits = value["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| {
                query.kind == MediaKind::Movie
                    || query
                        .season
                        .is_none_or(|season| itunes_season(item) == Some(season))
                        && query.episode.is_none_or(|episode| {
                            integer(item, "trackNumber") == Some(episode as i64)
                        })
            })
            .filter_map(|item| {
                let title = if query.kind == MediaKind::Movie {
                    string(item, "trackName")
                } else {
                    string(item, "artistName")
                };
                Some(SearchHit {
                    provider: Provider::ITunes,
                    kind: query.kind,
                    id: item.get("trackId")?.as_i64()?.to_string(),
                    title: nonempty(title)?.to_owned(),
                    subtitle: if query.kind == MediaKind::Movie {
                        string(item, "releaseDate").chars().take(10).collect()
                    } else {
                        string(item, "trackName").to_owned()
                    },
                    overview: string(item, "longDescription")
                        .or_if_empty(string(item, "shortDescription"))
                        .to_owned(),
                    thumbnail_url: itunes_artwork(string(item, "artworkUrl100"), 300),
                })
            })
            .take(100)
            .collect();
        Ok(hits)
    }

    fn resolve_itunes(&self, hit: &SearchHit, query: &SearchQuery) -> Result<MetadataResult> {
        let mut url = Url::parse(&self.itunes_base)?.join("lookup")?;
        url.query_pairs_mut()
            .append_pair("id", &hit.id)
            .append_pair("country", &query.country.to_ascii_uppercase());
        let value = self.get_json(url, None, Provider::ITunes)?;
        let item = value["results"]
            .as_array()
            .and_then(|items| items.first())
            .context("Risultato non più disponibile su iTunes Store")?;
        let title = string(item, "trackName");
        let description =
            string(item, "longDescription").or_if_empty(string(item, "shortDescription"));
        let mut fields = base_fields(
            title,
            description,
            &string(item, "releaseDate")
                .chars()
                .take(10)
                .collect::<String>(),
        );
        insert(&mut fields, "genre", string(item, "primaryGenreName"));
        insert(
            &mut fields,
            "content_rating",
            string(item, "contentAdvisoryRating"),
        );
        if hit.kind == MediaKind::Movie {
            insert(&mut fields, "director", string(item, "artistName"));
        } else {
            insert(&mut fields, "show", string(item, "artistName"));
            if let Some(season) = itunes_season(item).or(query.season) {
                fields.insert("season_number".into(), season.to_string());
            }
            if let Some(episode) = integer(item, "trackNumber")
                .map(|value| value as u32)
                .or(query.episode)
            {
                fields.insert("episode_sort".into(), episode.to_string());
            }
        }
        fields.insert("provider".into(), Provider::ITunes.label().into());
        fields.insert("provider_id".into(), hit.id.clone());
        Ok(MetadataResult {
            provider: Provider::ITunes,
            fields,
            artwork_url: itunes_artwork(string(item, "artworkUrl100"), 1200)
                .or_else(|| hit.thumbnail_url.clone()),
            source_url: nonempty(string(item, "trackViewUrl")).map(str::to_owned),
            attribution: Provider::ITunes.attribution().into(),
        })
    }
}

trait EmptyFallback<'a> {
    fn or_if_empty(self, fallback: &'a str) -> &'a str;
}

impl<'a> EmptyFallback<'a> for &'a str {
    fn or_if_empty(self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn nonempty(value: &str) -> Option<&str> {
    (!value.trim().is_empty()).then_some(value)
}

fn insert(fields: &mut BTreeMap<String, String>, key: &str, value: impl AsRef<str>) {
    if let Some(value) = nonempty(value.as_ref()) {
        fields.insert(key.into(), value.into());
    }
}

fn base_fields(title: &str, description: &str, date: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    insert(&mut fields, "title", title);
    insert(&mut fields, "description", description);
    insert(&mut fields, "date", date);
    fields
}

fn joined_names(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ")
}

fn apple_storefront(country: &str) -> Option<&'static str> {
    match country.to_ascii_uppercase().as_str() {
        "IT" => Some("143450"),
        "US" => Some("143441"),
        "GB" => Some("143444"),
        "DE" => Some("143443"),
        "FR" => Some("143442"),
        "ES" => Some("143454"),
        "CA" => Some("143455"),
        "AU" => Some("143460"),
        "JP" => Some("143462"),
        _ => None,
    }
}

fn apple_image(value: &Value, key: &str, width: u32, height: u32) -> Option<String> {
    let template = value
        .pointer(&format!("/images/{key}/url"))
        .and_then(Value::as_str)?;
    Some(
        template
            .replace("{w}", &width.to_string())
            .replace("{h}", &height.to_string())
            .replace("{c}", "")
            .replace("{f}", "jpg"),
    )
}

fn apple_date(value: Option<&Value>) -> String {
    let Some(milliseconds) = value.and_then(Value::as_f64) else {
        return String::new();
    };
    date_from_unix_days((milliseconds / 86_400_000.0).floor() as i64)
}

fn date_from_unix_days(days: i64) -> String {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn apple_roles(roles: &[Value], types: &[&str]) -> String {
    roles
        .iter()
        .filter(|role| types.contains(&string(role, "type")))
        .filter_map(|role| nonempty(string(role, "personName")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn tmdb_image(path: Option<&Value>, size: &str) -> Option<String> {
    let path = path.and_then(Value::as_str)?;
    Some(format!("https://image.tmdb.org/t/p/{size}{path}"))
}

fn tmdb_cast(value: &Value) -> String {
    value
        .pointer("/credits/cast")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(20)
        .filter_map(|person| nonempty(string(person, "name")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn tmdb_crew(value: &Value, job: &str) -> String {
    value
        .pointer("/credits/crew")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|person| string(person, "job") == job)
        .filter_map(|person| nonempty(string(person, "name")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn tmdb_department(value: &Value, department: &str) -> String {
    value
        .pointer("/credits/crew")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|person| string(person, "department") == department)
        .filter_map(|person| nonempty(string(person, "name")))
        .collect::<Vec<_>>()
        .join(", ")
}

fn absolute_image(value: Option<&Value>) -> Option<String> {
    let path = value.and_then(Value::as_str)?;
    if path.starts_with("https://") {
        Some(path.into())
    } else if path.starts_with('/') {
        Some(format!("https://artworks.thetvdb.com{path}"))
    } else {
        Some(format!("https://artworks.thetvdb.com/banners/{path}"))
    }
}

fn tvdb_language(locale: &str) -> Option<&'static str> {
    language::normalize(locale.split(['-', '_']).next().unwrap_or(locale))
}

fn tvdb_poster(value: &Value) -> Option<String> {
    value
        .get("artworks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|artwork| {
            integer(artwork, "type") == Some(2) || string(artwork, "image").contains("poster")
        })
        .and_then(|artwork| absolute_image(artwork.get("image")))
}

fn tvdb_people(value: &Value, types: &[i64]) -> String {
    value
        .get("characters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|person| integer(person, "type").is_some_and(|kind| types.contains(&kind)))
        .filter_map(|person| {
            nonempty(string(person, "personName")).or_else(|| nonempty(string(person, "name")))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn itunes_artwork(url: &str, size: u32) -> Option<String> {
    nonempty(url).map(|url| url.replace("100x100bb", &format!("{size}x{size}bb")))
}

fn itunes_season(value: &Value) -> Option<u32> {
    let name = string(value, "collectionName").to_ascii_lowercase();
    ["season ", "stagione ", "saison ", "staffel "]
        .into_iter()
        .find_map(|marker| {
            name.rsplit_once(marker)
                .and_then(|(_, number)| number.split_whitespace().next())
                .and_then(|number| number.parse().ok())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_names_and_aliases_are_stable() {
        assert_eq!(
            Provider::ALL.map(Provider::label),
            ["Apple TV", "TheMovieDB", "TheTVDB", "iTunes Store"]
        );
        assert_eq!("apple-tv".parse::<Provider>().unwrap(), Provider::AppleTv);
        assert_eq!("tmdb".parse::<Provider>().unwrap(), Provider::Tmdb);
    }

    #[test]
    fn apple_dates_and_images_are_normalized() {
        let item = json!({
            "releaseDate": 1631689200000.0,
            "images": { "coverArt": { "url": "https://example.test/{w}x{h}{c}.{f}" } }
        });
        assert_eq!(apple_date(item.get("releaseDate")), "2021-09-15");
        assert_eq!(
            apple_image(&item, "coverArt", 1200, 1800).unwrap(),
            "https://example.test/1200x1800.jpg"
        );
    }

    #[test]
    fn artwork_urls_are_upgraded() {
        assert_eq!(
            itunes_artwork("https://example.test/100x100bb.jpg", 1200).unwrap(),
            "https://example.test/1200x1200bb.jpg"
        );
        assert_eq!(
            tmdb_image(Some(&json!("/poster.jpg")), "original").unwrap(),
            "https://image.tmdb.org/t/p/original/poster.jpg"
        );
        assert_eq!(tvdb_language("it-IT"), Some("ita"));
        assert_eq!(tvdb_language("en_US"), Some("eng"));
    }

    #[test]
    fn query_validation_rejects_invalid_input() {
        let mut query = SearchQuery::italian(Provider::AppleTv, MediaKind::Movie, "");
        assert!(query.validate().is_err());
        query.term = "Dune".into();
        query.country = "Italia".into();
        assert!(query.validate().is_err());
    }
}
