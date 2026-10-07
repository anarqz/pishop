//! JSON API for sources, the file explorer and the copy queue.

use std::path::PathBuf;

use axum::extract::{Path, Query};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::sources::{self, PublicSource, Source};
use crate::{catalog, jobs, localfs, smbfs};
use crate::tr;

pub fn router() -> Router {
    Router::new()
        .route("/api/sources", get(list_sources).post(save_source))
        .route("/api/sources/test", post(test_source))
        .route("/api/sources/{id}", delete(delete_source))
        .route("/api/sources/{id}/list", get(list_remote))
        .route("/api/local/places", get(|| async { Json(localfs::places()) }))
        .route("/api/local/list", get(list_local))
        .route("/api/local/mkdir", post(mkdir))
        .route("/api/local/space", get(local_space))
        .route("/api/jobs", get(|| async { Json(jobs::list()) }).post(create_jobs))
        .route("/api/jobs/clear", post(|| async { jobs::remove(None); StatusCode::NO_CONTENT }))
        .route("/api/jobs/{id}/{action}", post(job_action))
        .route("/api/fonts/{file}", get(font))
        .route("/api/discover", get(discover_page))
        .route("/api/game/info", get(game_info))
        .route("/api/game/trailer", get(game_trailer))
        .route("/api/steam/open", post(steam_open))
        .route("/api/steam/art", get(steam_art))
        .route("/api/catalog/context", get(catalog_context))
        .route("/api/catalog/config", get(catalog_config).post(catalog_save))
        .route("/api/catalog/config/test", post(catalog_test))
        .route("/api/services/test", post(service_test))
        .route("/api/catalog/search", get(catalog_search))
        .route("/api/catalog/art", get(catalog_art))
        .route("/api/catalog/img", get(catalog_img))
        .route("/api/catalog/release/{id}", get(catalog_details))
        .route("/api/catalog/release/{id}/files", get(catalog_files))
        .route("/api/catalog/release/{id}/download", post(catalog_download))
        .merge(crate::archive::router())
        .merge(crate::compat::router())
        .merge(crate::exeguess::router())
        .merge(crate::install::router())
        .merge(crate::winetricks::router())
        .merge(crate::update::router())
        .merge(crate::vpn::router())
        .merge(crate::games::router())
        .merge(crate::patches::router())
        .route("/api/library", get(|| async { Json(crate::library::all()) }))
        .route("/api/focus", get(|| async { Json(json!({ "focused": crate::focus::focused() })) }))
        .route("/api/settings", get(|| async { Json(crate::settings::get()) }).post(save_settings))
        .route("/api/services/export", post(services_export))
        .route("/api/services/import", get(|| async { Json(crate::services_file::candidates()) }).post(services_import))
        .route("/api/services/import-url", post(services_import_url))
        .route("/api/torrent/limits", get(|| async { Json(crate::torrent::limits()) }).post(torrent_limits))
        .route(
            "/api/library/{hash}",
            delete(|Path(hash): Path<String>| async move {
                crate::library::forget_download(&hash);
                StatusCode::NO_CONTENT
            }),
        )
}

fn err(status: StatusCode, msg: impl std::fmt::Display) -> Response {
    (status, Json(json!({ "error": msg.to_string() }))).into_response()
}

async fn list_sources() -> Json<Vec<PublicSource>> {
    Json(sources::all().iter().map(PublicSource::from).collect())
}

/// Fills a blank password from the stored source (the UI never sees it).
fn with_stored_password(mut s: Source) -> Source {
    if s.password.is_empty() {
        if let Some(old) = sources::get(&s.id) {
            s.password = old.password;
        }
    }
    s
}

async fn save_source(Json(s): Json<Source>) -> Response {
    let s = sources::normalize(s);
    if s.host.is_empty() || s.share.is_empty() {
        return err(StatusCode::BAD_REQUEST, tr!("enter the server and the share", "informe o servidor e o compartilhamento"));
    }
    match sources::upsert(s) {
        Ok(saved) => Json(PublicSource::from(&saved)).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn test_source(Json(s): Json<Source>) -> Response {
    let s = with_stored_password(sources::normalize(s));
    if s.host.is_empty() || s.share.is_empty() {
        return err(StatusCode::BAD_REQUEST, tr!("enter the server and the share", "informe o servidor e o compartilhamento"));
    }
    match smbfs::test(&s).await {
        Ok(n) => Json(json!({ "ok": true, "entries": n })).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn delete_source(Path(id): Path<String>) -> Response {
    match sources::remove(&id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct PathQuery {
    #[serde(default)]
    path: String,
}

async fn list_remote(Path(id): Path<String>, Query(q): Query<PathQuery>) -> Response {
    let Some(s) = sources::get(&id) else { return err(StatusCode::NOT_FOUND, tr!("source not found", "fonte não encontrada")) };
    let started = std::time::Instant::now();
    match smbfs::list(&s, &q.path).await {
        Ok(entries) => {
            crate::log!("smb: {} entradas em {:?} ({} ms)", entries.len(), q.path, started.elapsed().as_millis());
            Json(json!({ "path": q.path.trim_matches('/'), "entries": entries })).into_response()
        }
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn list_local(Query(q): Query<PathQuery>) -> Response {
    let path = PathBuf::from(&q.path);
    if !path.is_absolute() {
        return err(StatusCode::BAD_REQUEST, tr!("invalid path", "caminho inválido"));
    }
    match tokio::task::spawn_blocking(move || localfs::list(&path)).await {
        Ok(Ok(entries)) => {
            let space = localfs::disk_space(std::path::Path::new(&q.path));
            Json(json!({
                "path": q.path,
                "entries": entries,
                "free": space.map(|s| s.0),
                "total": space.map(|s| s.1),
            }))
            .into_response()
        }
        Ok(Err(e)) => err(StatusCode::BAD_REQUEST, tr!("couldn't open {}: {e}", "não foi possível abrir {}: {e}", q.path)),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// Free and total space of the disk a path is on (the nearest folder that
/// exists, for one about to be created), and that disk's name.
async fn local_space(Query(q): Query<PathQuery>) -> Response {
    let path = PathBuf::from(&q.path);
    if !path.is_absolute() {
        return err(StatusCode::BAD_REQUEST, tr!("invalid path", "caminho inválido"));
    }
    let existing = path.ancestors().find(|a| a.exists()).unwrap_or(std::path::Path::new("/")).to_path_buf();
    let space = localfs::disk_space(&existing);
    Json(json!({
        "path": q.path,
        "free": space.map(|s| s.0),
        "total": space.map(|s| s.1),
        "disk": crate::install::disk_label(&existing),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct MkdirReq {
    path: String,
}

async fn mkdir(Json(r): Json<MkdirReq>) -> Response {
    let p = PathBuf::from(&r.path);
    if !p.is_absolute() {
        return err(StatusCode::BAD_REQUEST, tr!("invalid path", "caminho inválido"));
    }
    match std::fs::create_dir_all(&p) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
struct NewJobs {
    source_id: String,
    dest: String,
    items: Vec<jobs::NewItem>,
}

async fn create_jobs(Json(r): Json<NewJobs>) -> Response {
    if r.items.is_empty() {
        return err(StatusCode::BAD_REQUEST, tr!("nothing selected", "nada selecionado"));
    }
    match jobs::enqueue(&r.source_id, &r.dest, r.items) {
        Ok(ids) => Json(json!({ "ids": ids })).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

async fn job_action(Path((id, action)): Path<(u64, String)>) -> Response {
    match action.as_str() {
        "cancel" => jobs::cancel(id),
        "retry" => jobs::retry(id),
        "remove" => jobs::remove(Some(id)),
        _ => return err(StatusCode::NOT_FOUND, tr!("unknown action", "ação desconhecida")),
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Steam's UI font (Motiva Sans), served from the user's own Steam install
/// when present; the UI falls back to Noto Sans otherwise.
async fn font(Path(file): Path<String>) -> Response {
    let allowed = ["motiva-sans.ttf", "motiva-sans-medium.ttf", "motiva-sans-bold.ttf", "motiva-sans-light.ttf"];
    if !allowed.contains(&file.as_str()) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = localfs::home().join(".local/share/Steam/steamapps/common/SteamVR/resources/webinterface/fonts").join(&file);
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, "font/ttf"), (header::CACHE_CONTROL, "public, max-age=604800")],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---------- store catalog ----------

async fn catalog_config() -> Json<serde_json::Value> {
    Json(catalog::public_config())
}

async fn catalog_save(Json(u): Json<catalog::ConfigUpdate>) -> Response {
    match catalog::update_config(u) {
        Ok(_) => Json(catalog::public_config()).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct ServiceTest {
    service: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    cdn: String,
}

/// Settings "Testar" for TheGamesDB / isitcracked (blank key → stored one).
async fn service_test(Json(t): Json<ServiceTest>) -> Response {
    let c = catalog::config();
    let pick = |v: &str, stored: &str| if v.trim().is_empty() { stored.to_string() } else { v.trim().to_string() };
    let r = match t.service.as_str() {
        "tgdb" => crate::tgdb::test(&pick(&t.key, &c.tgdb_key)).await,
        "iic" => crate::discover::test(&pick(&t.url, &c.iic_url), &pick(&t.key, &c.iic_key), &pick(&t.cdn, &c.iic_cdn))
            .await
            .map(|total| json!({ "total": total })),
        "tpb" => crate::tpb::search(&pick(&t.url, &c.tpb_url), "linux", "0").await.map(|hits| json!({ "total": hits.len() })),
        _ => return err(StatusCode::NOT_FOUND, tr!("unknown service", "serviço desconhecido")),
    };
    match r {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn catalog_test(Json(mut c): Json<catalog::Config>) -> Response {
    if c.prowlarr_key.trim().is_empty() {
        c.prowlarr_key = catalog::config().prowlarr_key;
    }
    c.prowlarr_url = c.prowlarr_url.trim().trim_end_matches('/').to_string();
    if !c.prowlarr_url.starts_with("http") {
        c.prowlarr_url = format!("http://{}", c.prowlarr_url);
    }
    match catalog::test(&c).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    kind: String,
    /// Skip the cached answer for this search.
    #[serde(default)]
    fresh: bool,
}

async fn catalog_search(Query(q): Query<SearchQuery>) -> Response {
    let started = std::time::Instant::now();
    match catalog::search(q.q.trim(), &q.kind, q.fresh).await {
        Ok(out) => {
            crate::log!(
                "catálogo: {:?} ({}) → {} resultados em {} ms{}",
                q.q,
                q.kind,
                out.results.len(),
                started.elapsed().as_millis(),
                if out.warnings.is_empty() { String::new() } else { format!(" · avisos: {}", out.warnings.join("; ")) }
            );
            Json(out).into_response()
        }
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

/// A Steam game's library art (cover, hero), for the Store's match picked by hand.
async fn steam_art(Query(q): Query<AppidQuery>) -> Response {
    match crate::steam_store::library_art(q.appid.trim()).await {
        Some(a) => Json(json!({ "cover": a.cover_2x.or(a.cover), "hero": a.hero_2x.or(a.hero) })).into_response(),
        None => Json(json!({ "cover": null, "hero": null })).into_response(),
    }
}

#[derive(Deserialize)]
struct NameQuery {
    name: String,
}

async fn catalog_art(Query(q): Query<NameQuery>) -> Json<Option<catalog::Art>> {
    Json(catalog::art(&q.name).await)
}

#[derive(Deserialize)]
struct UrlQuery {
    u: String,
}

async fn catalog_img(Query(q): Query<UrlQuery>) -> Response {
    match catalog::image(&q.u).await {
        Ok((bytes, mime)) => (
            [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "public, max-age=2592000".to_string())],
            bytes,
        )
            .into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct DetailsQuery {
    #[serde(default = "yes")]
    art: bool,
}

fn yes() -> bool {
    true
}

async fn catalog_details(Path(id): Path<String>, Query(q): Query<DetailsQuery>) -> Response {
    match catalog::details(&id, q.art).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, format!("{e:#}")),
    }
}

async fn catalog_files(Path(id): Path<String>) -> Response {
    match catalog::files(&id).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct DownloadReq {
    dest: Option<String>,
    #[serde(default)]
    hint: Option<crate::library::Hint>,
}

async fn catalog_download(Path(id): Path<String>, Json(r): Json<DownloadReq>) -> Response {
    match catalog::download(&id, r.dest.filter(|d| !d.trim().is_empty()), r.hint).await {
        Ok(v) => Json(v).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

async fn save_settings(Json(s): Json<crate::settings::Settings>) -> Response {
    match crate::settings::save(s) {
        Ok(s) => Json(s).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

async fn services_export() -> Response {
    match crate::services_file::export() {
        Ok(path) => Json(json!({ "path": path })).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct ImportReq {
    path: String,
}

async fn services_import(Json(r): Json<ImportReq>) -> Response {
    match crate::services_file::import(&r.path) {
        Ok(names) => Json(json!({ "imported": names })).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct ImportUrlReq {
    url: String,
}

async fn services_import_url(Json(r): Json<ImportUrlReq>) -> Response {
    match crate::services_file::import_url(&r.url).await {
        Ok(names) => Json(json!({ "imported": names })).into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, format!("{e:#}")),
    }
}

async fn torrent_limits(Json(l): Json<crate::torrent::Limits>) -> Response {
    match crate::torrent::set_limits(l) {
        Ok(l) => Json(l).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct DiscoverQuery {
    search: Option<String>,
    #[serde(default)]
    offset: u64,
    limit: Option<u64>,
}

async fn discover_page(Query(q): Query<DiscoverQuery>) -> Response {
    match crate::discover::page(q.search.as_deref(), q.offset, q.limit.unwrap_or(30)).await {
        Ok(p) => Json(p).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct GameQuery {
    name: String,
    /// Include TheGamesDB metadata (costs monthly quota the first time).
    #[serde(default)]
    tgdb: bool,
    /// Steam appid: store page data as the fallback when TGDB has nothing.
    #[serde(default)]
    appid: String,
}

async fn game_info(Query(q): Query<GameQuery>) -> Json<serde_json::Value> {
    let art = catalog::art(&q.name).await;
    let hero = async {
        match &art {
            Some(a) => catalog::hero(a).await,
            None => None,
        }
    };
    let details = async {
        if !q.tgdb {
            return (None, None);
        }
        // Steam store first: faster, Portuguese, and no TheGamesDB quota.
        let appid = if q.appid.is_empty() { crate::steam_store::find_appid(&q.name).await } else { Some(q.appid.clone()) };
        let steam = match &appid {
            Some(id) => crate::steam_store::details(id).await,
            None => None,
        };
        let tgdb = if steam.as_ref().is_some_and(|s| s.overview.is_some()) {
            None
        } else {
            crate::tgdb::details(art.as_ref().map(|a| a.name.as_str()).unwrap_or(&q.name)).await
        };
        (steam, tgdb)
    };
    let (hero, (steam, tgdb)) = tokio::join!(hero, details);
    Json(json!({ "art": art, "hero": hero, "steam": steam, "tgdb": tgdb }))
}

#[derive(Deserialize)]
struct AppidQuery {
    appid: String,
}

/// "Abrir na Steam": the store page in the Steam client.
async fn steam_open(Query(q): Query<AppidQuery>) -> Response {
    match crate::steam_store::open_store_page(q.appid.trim()) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn game_trailer(Query(q): Query<GameQuery>) -> Response {
    match crate::trailer::find(&q.name).await {
        Ok(t) => Json(t).into_response(),
        Err(e) => err(StatusCode::BAD_GATEWAY, format!("{e:#}")),
    }
}

#[derive(Deserialize)]
struct ContextQuery {
    q: String,
    /// Skip the isitcracked lookup (already known when coming from Discover).
    #[serde(default)]
    no_crack: bool,
}

/// Store header for any search: SteamGridDB art + isitcracked status.
async fn catalog_context(Query(c): Query<ContextQuery>) -> Json<serde_json::Value> {
    let (art, crack) = tokio::join!(catalog::art(&c.q), async {
        if c.no_crack { None } else { crate::discover::best_match(&c.q).await }
    });
    let hero = match &art {
        Some(a) => catalog::hero(a).await,
        None => None,
    };
    Json(json!({ "art": art, "hero": hero, "crack": crack }))
}
