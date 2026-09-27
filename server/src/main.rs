use std::net::SocketAddr;

use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, header::CACHE_CONTROL},
    middleware::{self, Next},
    response::{Html, Response},
    routing::get,
};

const INDEX_HTML: &str = include_str!("../../frontend/index.html");
const STYLES_CSS: &str = include_str!("../../frontend/styles.css");
const MAIN_JS: &str = include_str!("../../frontend/main.js");
const AUDIO_JS: &str = include_str!("../../frontend/audio.js");
const HRIR_PLOT_JS: &str = include_str!("../../frontend/hrir-plot.js");
const AUDIO_WORKLET_BUNDLE_JS: &str = include_str!("../../frontend/audio-worklet-bundle.js");
const HRTF_DATASET: &[u8] = include_bytes!("../../frontend/assets/mit-kemar.bhrtf");
const WASM_BINDGEN_JS: &str = include_str!("../../frontend/pkg/binaural_explorer_web.js");
const WEB_WASM: &[u8] = include_bytes!("../../frontend/pkg/binaural_explorer_web_bg.wasm");
const AUDIO_WORKLET_WASM: &[u8] =
    include_bytes!("../../frontend/pkg/binaural_audio_worklet_nomodule_bg.wasm");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = app();
    let address = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Binaural HRTF Explorer listening on http://{address}");
    axum::serve(listener, app).await?;
    Ok(())
}

fn app() -> Router {
    asset_routes().layer(middleware::from_fn(disable_static_asset_cache))
}

fn asset_routes() -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route(
            "/styles.css",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    STYLES_CSS,
                )
            }),
        )
        .route(
            "/main.js",
            get(|| async {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/javascript; charset=utf-8",
                    )],
                    MAIN_JS,
                )
            }),
        )
        .route(
            "/audio.js",
            get(|| async {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/javascript; charset=utf-8",
                    )],
                    AUDIO_JS,
                )
            }),
        )
        .route(
            "/hrir-plot.js",
            get(|| async {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/javascript; charset=utf-8",
                    )],
                    HRIR_PLOT_JS,
                )
            }),
        )
        .route(
            "/audio-worklet-bundle.js",
            get(|| async {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/javascript; charset=utf-8",
                    )],
                    AUDIO_WORKLET_BUNDLE_JS,
                )
            }),
        )
        .route(
            "/assets/mit-kemar.bhrtf",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                    HRTF_DATASET,
                )
            }),
        )
        .route(
            "/pkg/binaural_explorer_web.js",
            get(|| async {
                (
                    [(
                        axum::http::header::CONTENT_TYPE,
                        "text/javascript; charset=utf-8",
                    )],
                    WASM_BINDGEN_JS,
                )
            }),
        )
        .route(
            "/pkg/binaural_explorer_web_bg.wasm",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/wasm")],
                    WEB_WASM,
                )
            }),
        )
        .route(
            "/pkg/binaural_audio_worklet_nomodule_bg.wasm",
            get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/wasm")],
                    AUDIO_WORKLET_WASM,
                )
            }),
        )
}

async fn disable_static_asset_cache(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_routes_can_be_constructed() {
        let _application = app();
    }

    #[test]
    fn embedded_browser_assets_include_interaction_and_audio_features() {
        assert!(INDEX_HTML.contains("max=\"12\""));
        assert!(INDEX_HTML.contains("id=\"volume-warning\""));
        assert!(INDEX_HTML.contains("id=\"loop\""));
        assert!(INDEX_HTML.contains("id=\"audio-source-panel\""));
        assert!(INDEX_HTML.contains("id=\"info-dialog\""));
        assert!(INDEX_HTML.contains("Every direction is relative to the head"));
        assert!(INDEX_HTML.contains("id=\"playback-progress\""));
        assert!(INDEX_HTML.contains("up to 2 GiB"));
        assert!(INDEX_HTML.contains("data-camera-preset=\"front\""));
        assert!(INDEX_HTML.contains("data-source-preset=\"front\""));
        assert!(INDEX_HTML.contains("id=\"azimuth-number\""));
        assert!(INDEX_HTML.contains("id=\"interpolation-method\""));
        assert!(INDEX_HTML.contains("value=\"spherical-triangle\""));
        assert!(INDEX_HTML.contains("value=\"time-aligned-spherical-triangle\""));
        assert!(INDEX_HTML.contains("value=\"minimum-phase\""));
        assert!(INDEX_HTML.contains("value=\"frequency-domain\""));
        assert!(INDEX_HTML.contains("value=\"spherical-harmonics\""));
        assert!(INDEX_HTML.contains("data-camera-preset=\"bottom\""));
        assert!(INDEX_HTML.contains("id=\"hrir-plot\""));
        assert!(INDEX_HTML.contains("data-display-layer=\"measurements\""));
        assert!(INDEX_HTML.contains("left-sidebar"));
        assert!(INDEX_HTML.contains("right-sidebar"));
        assert!(INDEX_HTML.contains("+X right"));
        assert!(INDEX_HTML.contains("right-drag orbit"));
        assert!(!INDEX_HTML.contains("two fingers orbit/pinch"));
        assert!(INDEX_HTML.contains("href=\"./styles.css"));
        assert!(INDEX_HTML.contains("src=\"./main.js"));
        assert!(MAIN_JS.contains("navigate_camera"));
        assert!(MAIN_JS.contains("showModal"));
        assert!(MAIN_JS.contains("set_display_layer"));
        assert!(MAIN_JS.contains("set_interpolation_method"));
        assert!(MAIN_JS.contains("renderHrirPlot"));
        assert!(MAIN_JS.contains("new URL(\"./assets/mit-kemar.bhrtf\""));
        assert!(AUDIO_JS.contains("mapHeadRelativeHrirToHeadphones"));
        assert!(AUDIO_JS.contains("MAX_VOLUME_DECIBELS = 12"));
        assert!(AUDIO_JS.contains("playSpatializedMono"));
        assert!(AUDIO_JS.contains("playSpatializedMedia"));
        assert!(AUDIO_JS.contains("createMediaElementSource"));
        assert!(!AUDIO_JS.contains("decodeAudioData"));
        assert!(AUDIO_JS.contains("validateAudioFile"));
        assert!(AUDIO_JS.contains("./audio-worklet-bundle.js"));
        assert!(AUDIO_WORKLET_BUNDLE_JS.contains("let wasm_bindgen"));
        assert!(AUDIO_WORKLET_BUNDLE_JS.contains("TextDecoder"));
        assert!(AUDIO_WORKLET_BUNDLE_JS.contains("binaural-hrtf-processor"));
        assert!(!AUDIO_WORKLET_WASM.is_empty());
    }
}
