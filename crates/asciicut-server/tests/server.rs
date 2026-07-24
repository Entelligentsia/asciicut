//! In-process integration tests for the `asciicut-server` HTTP surface.
//!
//! Every endpoint is driven through the full axum request path with
//! `tower::ServiceExt::oneshot` — no real socket is bound, so the tests are fast
//! and deterministic. A tiny fixture cast seeds [`AppState`]; `/api/save` writes
//! into a `tempfile::TempDir` so the server-derived path is asserted on disk.

use std::path::PathBuf;
use std::sync::Arc;

use asciicut_core::Cast;
use asciicut_server::{router, AppState};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt; // for `oneshot`

/// A small, well-formed source cast: a 20x3 terminal with two output events.
const FIXTURE_CAST: &str = concat!(
    "{\"version\": 2, \"width\": 20, \"height\": 3}\n",
    "[0.5, \"o\", \"hello\"]\n",
    "[1.5, \"o\", \" world\"]\n",
);

/// A minimal valid edit project over the fixture, one keep-segment.
const FIXTURE_PROJECT: &str = concat!(
    "{\"source\":\"fixture.cast\",",
    "\"segments\":[{\"srcStart\":0.0,\"srcEnd\":2.0,\"speed\":1.0}]}",
);

fn state_at(dir: PathBuf) -> Arc<AppState> {
    let cast = Cast::parse(FIXTURE_CAST).expect("fixture cast parses");
    Arc::new(AppState::new(cast, dir.join("fixture.cast")))
}

fn state() -> Arc<AppState> {
    state_at(PathBuf::from("."))
}

async fn body_string(response: axum::response::Response) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes();
    String::from_utf8(bytes.to_vec()).expect("body is utf-8")
}

#[tokio::test]
async fn frame_returns_grid_at_t() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/frame?t=1.0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["width"], 20);
    assert_eq!(json["height"], 3);
    // First row starts with the replayed output at T=1.0 ("hello", before " world").
    assert!(json["text"][0].as_str().unwrap().starts_with("hello"));
    assert_eq!(json["text"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn frame_missing_t_is_400() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/frame")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn frame_non_numeric_t_is_400() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/frame?t=notanumber")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn thumbs_default_count() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/thumbs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 12); // DEFAULT_THUMBS
                               // Each element carries its sampled time and a frame DTO.
    assert!(arr[0]["t"].is_number());
    assert_eq!(arr[0]["frame"]["height"], 3);
}

#[tokio::test]
async fn thumbs_count_is_clamped() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/thumbs?count=100000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 240); // MAX_THUMBS
}

#[tokio::test]
async fn thumbs_count_one_is_single_frame() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/thumbs?count=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["t"], 0.0);
}

#[tokio::test]
async fn activity_default_bucket() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/activity")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    // DEFAULT_BUCKET_SECS = 0.25; fixture's last event is at t=1.5 →
    // ceil(1.5/0.25) = 6 buckets. "hello" (5) lands in bucket 2, " world" (6) in
    // bucket 6 but clamps to the last bucket (index 5).
    assert!((json["bucket_secs"].as_f64().unwrap() - 0.25).abs() < 1e-12);
    let buckets = json["buckets"].as_array().unwrap();
    assert_eq!(buckets.len(), 6);
    assert_eq!(buckets[2], 5);
    assert_eq!(buckets[5], 6);
}

#[tokio::test]
async fn activity_explicit_bucket() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/activity?bucket=0.5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    // bucket=0.5 → ceil(1.5/0.5) = 3 buckets. "hello" (5) at t=0.5 → bucket 1,
    // " world" (6) at t=1.5 floors to bucket 3, clamps to last (index 2).
    assert!((json["bucket_secs"].as_f64().unwrap() - 0.5).abs() < 1e-12);
    let buckets = json["buckets"].as_array().unwrap();
    assert_eq!(buckets.len(), 3);
    assert_eq!(buckets[1], 5);
    assert_eq!(buckets[2], 6);
}

#[tokio::test]
async fn activity_eventless_cast_is_empty_waveform() {
    // A header-only cast has no events → an empty waveform array.
    let cast = Cast::parse("{\"version\": 2, \"width\": 20, \"height\": 3}\n")
        .expect("header-only cast parses");
    let state = Arc::new(AppState::new(cast, PathBuf::from("empty.cast")));

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/api/activity")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(json["buckets"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn activity_non_finite_bucket_falls_back_to_default() {
    // A non-finite bucket must NOT be pinned to the MIN floor; it falls through
    // to the core guard's DEFAULT_BUCKET_SECS (0.25) fallback.
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/activity?bucket=inf")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert!((json["bucket_secs"].as_f64().unwrap() - 0.25).abs() < 1e-12);
    assert_eq!(json["buckets"].as_array().unwrap().len(), 6);
}

#[tokio::test]
async fn activity_tiny_bucket_is_clamped_not_crash() {
    // Regression guard: a tiny positive bucket would saturate the core's bucket
    // count and `vec![0u64; count]` would abort the process. The MIN_BUCKET_SECS
    // clamp must bound it → 200 with a small, finite array.
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/activity?bucket=0.0000001")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    // Clamped to the 0.05 floor: ceil(1.5/0.05) = 30 buckets — bounded, not
    // billions. bucket_secs is reported as the clamped floor, not the tiny input.
    assert!((json["bucket_secs"].as_f64().unwrap() - 0.05).abs() < 1e-12);
    let buckets = json["buckets"].as_array().unwrap();
    assert_eq!(buckets.len(), 30);
}

#[tokio::test]
async fn compose_returns_v2_cast() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/compose")
                .body(Body::from(FIXTURE_PROJECT))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    // The composed document is a v2 asciicast: a header line then event lines.
    let first_line = body.lines().next().unwrap();
    let header: serde_json::Value = serde_json::from_str(first_line).unwrap();
    assert_eq!(header["version"], 2);
    assert!(body.lines().count() >= 2);
}

#[tokio::test]
async fn compose_malformed_project_is_400() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/compose")
                .body(Body::from("{ not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn save_writes_server_derived_path() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path().to_path_buf());
    let expected = dir.path().join("fixture.asciicut.json");

    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/save")
                .body(Body::from(FIXTURE_PROJECT))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(json["path"], expected.display().to_string());

    // The project was persisted verbatim to the server-derived path.
    let written = std::fs::read_to_string(&expected).unwrap();
    assert_eq!(written, FIXTURE_PROJECT);
}

#[tokio::test]
async fn save_rejects_malformed_project() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path().to_path_buf());

    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/save")
                .body(Body::from("{ not a project"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    // Nothing was written.
    assert!(!dir.path().join("fixture.asciicut.json").exists());
}

#[tokio::test]
async fn project_no_persisted_file_is_null() {
    // No `.asciicut.json` on disk → `project: null`, but the authoritative launch
    // source name is always reported.
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path().to_path_buf());

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/api/project")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(json["source"], "fixture.cast");
    assert!(json["project"].is_null());
}

#[tokio::test]
async fn project_echoes_persisted_file() {
    // A persisted `.asciicut.json` at the server-derived path is validated and
    // echoed verbatim (raw JSON value) alongside the source name.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fixture.asciicut.json"), FIXTURE_PROJECT).unwrap();
    let state = state_at(dir.path().to_path_buf());

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/api/project")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(json["source"], "fixture.cast");
    // The persisted project is echoed as a structured value, not a string.
    assert_eq!(json["project"]["source"], "fixture.cast");
    assert_eq!(json["project"]["segments"][0]["srcStart"], 0.0);
    assert_eq!(json["project"]["segments"][0]["srcEnd"], 2.0);
}

#[tokio::test]
async fn project_corrupt_persisted_file_is_422() {
    // A file that exists but fails validation surfaces a loud error rather than
    // being silently reported as absent (review advisory).
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fixture.asciicut.json"), "{ not a project").unwrap();
    let state = state_at(dir.path().to_path_buf());

    let response = router(state)
        .oneshot(
            Request::builder()
                .uri("/api/project")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn export_writes_project_and_composed_cast() {
    // Export writes BOTH `<stem>.asciicut.json` and `<stem>.composed.cast` to the
    // server-derived paths, and reports the composed byte count.
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path().to_path_buf());
    let project_path = dir.path().join("fixture.asciicut.json");
    let cast_path = dir.path().join("fixture.composed.cast");

    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/export")
                .body(Body::from(FIXTURE_PROJECT))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = serde_json::from_str(&body_string(response).await).unwrap();
    assert_eq!(json["projectPath"], project_path.display().to_string());
    assert_eq!(json["castPath"], cast_path.display().to_string());

    // Both files landed on disk. The project is the body verbatim.
    let written_project = std::fs::read_to_string(&project_path).unwrap();
    assert_eq!(written_project, FIXTURE_PROJECT);
    let written_cast = std::fs::read_to_string(&cast_path).unwrap();
    assert_eq!(json["bytes"].as_u64().unwrap() as usize, written_cast.len());
    // The composed export re-parses as a v2 cast.
    let header_line = written_cast.lines().next().unwrap();
    let header: serde_json::Value = serde_json::from_str(header_line).unwrap();
    assert_eq!(header["version"], 2);
}

#[tokio::test]
async fn export_composed_cast_is_byte_identical_to_compose() {
    // AC#3 byte-identity at the HTTP boundary: the composed `.cast` written by
    // `/api/export` equals the `/api/compose` response for the same project
    // (both funnel through core `compose().to_cast_string()`).
    let dir = tempfile::tempdir().unwrap();
    let cast_path = dir.path().join("fixture.composed.cast");

    // Compose the same project through the preview endpoint.
    let compose_response = router(state_at(dir.path().to_path_buf()))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/compose")
                .body(Body::from(FIXTURE_PROJECT))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(compose_response.status(), StatusCode::OK);
    let preview = body_string(compose_response).await;

    // Export the same project; the on-disk composed cast must byte-match.
    let export_response = router(state_at(dir.path().to_path_buf()))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/export")
                .body(Body::from(FIXTURE_PROJECT))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(export_response.status(), StatusCode::OK);

    let exported = std::fs::read_to_string(&cast_path).unwrap();
    assert_eq!(
        exported, preview,
        "exported .cast must equal the preview byte-for-byte"
    );
}

#[tokio::test]
async fn export_rejects_malformed_project() {
    let dir = tempfile::tempdir().unwrap();
    let state = state_at(dir.path().to_path_buf());

    let response = router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/export")
                .body(Body::from("{ not a project"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    // Neither file was written.
    assert!(!dir.path().join("fixture.asciicut.json").exists());
    assert!(!dir.path().join("fixture.composed.cast").exists());
}

#[tokio::test]
async fn spa_fallback_serves_index_for_root() {
    let response = router(state())
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    // Asserts a stable marker from the shipped Vite build shell (the SPA mount
    // point), not the removed placeholder `data-testid`.
    assert!(body.contains("<div id=\"root\">"));
}

#[tokio::test]
async fn spa_fallback_serves_index_for_unknown_route() {
    // A client-side route unknown to the server falls through to index.html.
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/editor/deep/link")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(body_string(response).await.contains("<div id=\"root\">"));
}
