#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!("Release desktop builds must enable custom-protocol to embed the local UI and exclude the development server URL.");

mod batch;
mod grants;
mod host;
pub mod native;
mod storage;

use design_edit::{Command, Mode};
use design_model::{Asset, AssetKind, Code, Font, MimeHint, PackagePolicy, ResolveState};
use design_render::{
    RenderOptions,
    resources::{MAX_FONT_BYTES, MAX_IMAGE_BYTES, ResourceStore, sha256_hex},
};
use host::{Host, HostResult, Mutation, PreviewMetadata, SessionView, SharedHost, error, mutate};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

async fn blocking<T: Send + 'static>(operation: impl FnOnce() -> HostResult<T> + Send + 'static) -> HostResult<T> {
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| error(Code::RenderFailed, "native operation was interrupted", "Retry the operation."))?
}

fn lock(shared: &SharedHost) -> HostResult<std::sync::MutexGuard<'_, Host>> {
    shared.lock().map_err(|_| {
        error(
            Code::RenderFailed,
            "document host is unavailable",
            "Restart the app and recover the last committed document.",
        )
    })
}

fn checkpoint(shared: &SharedHost, view: SessionView) -> HostResult<SessionView> {
    let mut host = lock(shared)?;
    let workspace = host.workspace.clone();
    let recovery = host.recovery.clone();
    let record = host.record_mut(&view.session_id)?;
    if record.checkpoint_revision != Some(record.session.revision()) {
        project_store::write_recovery(&record.workspace, record.session.document(), &record.resources).map_err(|i| vec![i])?;
        record.checkpoint_revision = Some(record.session.revision());
    }
    let relative = record
        .workspace
        .strip_prefix(&workspace)
        .map_err(|_| error(Code::SaveFailed, "recovery workspace is invalid", "Save the project explicitly."))?;
    let name = relative.to_str().ok_or_else(|| {
        error(
            Code::SaveFailed,
            "recovery workspace is not Unicode",
            "Save the project explicitly.",
        )
    })?;
    grants::DirectoryGrant::new(recovery)
        .and_then(|grant| grant.replace_control("latest", name.as_bytes()))
        .map_err(|i| vec![i])?;
    Ok(view)
}

#[tauri::command]
async fn open_fixture(state: State<'_, SharedHost>, fixture: Option<String>) -> HostResult<SessionView> {
    let shared = state.inner().clone();
    blocking(move || {
        let view = lock(&shared)?.open_fixture(fixture.as_deref().unwrap_or("retail-promo-template"))?;
        checkpoint(&shared, view)
    })
    .await
}

#[tauri::command]
async fn new_document(state: State<'_, SharedHost>) -> HostResult<SessionView> {
    let shared = state.inner().clone();
    blocking(move || {
        let view = lock(&shared)?.new_document()?;
        checkpoint(&shared, view)
    })
    .await
}

#[tauri::command]
fn read_session(state: State<'_, SharedHost>, session_id: String) -> HostResult<SessionView> {
    Ok(lock(state.inner())?.record(&session_id)?.view(&session_id))
}

#[tauri::command]
fn close_session(state: State<'_, SharedHost>, session_id: String) -> HostResult<()> {
    let mut host = lock(state.inner())?;
    host.record(&session_id)?;
    host.sessions.remove(&session_id);
    Ok(())
}

#[tauri::command]
async fn apply_edit(
    state: State<'_, SharedHost>,
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
    command: Command,
) -> HostResult<SessionView> {
    let shared = state.inner().clone();
    blocking(move || {
        let view = mutate(
            &mut *lock(&shared)?,
            &session_id,
            expected_revision,
            &transaction_id,
            Mutation::Edit(command),
        )?;
        checkpoint(&shared, view)
    })
    .await
}

#[tauri::command]
async fn undo_edit(
    state: State<'_, SharedHost>,
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
) -> HostResult<SessionView> {
    let shared = state.inner().clone();
    blocking(move || {
        let view = mutate(
            &mut *lock(&shared)?,
            &session_id,
            expected_revision,
            &transaction_id,
            Mutation::Undo,
        )?;
        checkpoint(&shared, view)
    })
    .await
}

#[tauri::command]
async fn redo_edit(
    state: State<'_, SharedHost>,
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
) -> HostResult<SessionView> {
    let shared = state.inner().clone();
    blocking(move || {
        let view = mutate(
            &mut *lock(&shared)?,
            &session_id,
            expected_revision,
            &transaction_id,
            Mutation::Redo,
        )?;
        checkpoint(&shared, view)
    })
    .await
}

#[tauri::command]
async fn set_mode(
    state: State<'_, SharedHost>,
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
    mode: Mode,
) -> HostResult<SessionView> {
    let shared = state.inner().clone();
    blocking(move || {
        let view = mutate(
            &mut *lock(&shared)?,
            &session_id,
            expected_revision,
            &transaction_id,
            Mutation::Mode(mode),
        )?;
        checkpoint(&shared, view)
    })
    .await
}

fn render_preview(shared: &SharedHost, session_id: &str, revision: u64, variant_id: &str, request_id: u64) -> HostResult<Vec<u8>> {
    render_preview_output(shared, session_id, revision, variant_id, request_id).map(|out| out.rgba)
}

fn render_preview_output(
    shared: &SharedHost,
    session_id: &str,
    revision: u64,
    variant_id: &str,
    request_id: u64,
) -> HostResult<design_render::RenderOutput> {
    let (doc, resources, _lease) = {
        let mut host = lock(shared)?;
        let record = host.record_mut(session_id)?;
        record.check_revision(revision)?;
        if record.latest_request.is_some_and(|last| request_id <= last) {
            return Err(error(
                Code::RevisionConflict,
                "preview requestId is stale",
                "Use a monotonically increasing preview request ID.",
            ));
        }
        record.latest_request = Some(request_id);
        host.snapshot(shared.clone(), session_id, revision)?
    };
    let out = design_render::render_preview_document(&doc, variant_id, &resources, RenderOptions::default())?;
    let mut host = lock(shared)?;
    let record = host.record_mut(session_id)?;
    record.check_revision(revision)?;
    if record.latest_request != Some(request_id) {
        return Err(error(
            Code::RevisionConflict,
            "preview was superseded",
            "Display only the newest requested revision.",
        ));
    }
    let stride = out
        .report
        .width
        .checked_mul(4)
        .ok_or_else(|| error(Code::ResourceLimit, "preview stride exceeds its limit", "Use a smaller artboard."))?;
    if out.rgba.len() != stride as usize * out.report.height as usize {
        return Err(error(
            Code::RenderFailed,
            "native preview payload does not match its dimensions",
            "Retry rendering the document.",
        ));
    }
    record.preview = Some(PreviewMetadata {
        document_revision: revision,
        request_id,
        width: out.report.width,
        height: out.report.height,
        stride,
        payload_length: out.rgba.len(),
        pixel_format: "RGBA8",
        color_space: "sRGB",
        alpha_type: "straight",
        quality: "exact",
        output_sha256: out.report.output_sha256.clone(),
        frame_sha256: sha256_hex(&out.rgba),
        issues: out.report.warnings.clone(),
    });
    Ok(out)
}

#[tauri::command]
async fn request_preview(
    state: State<'_, SharedHost>,
    session_id: String,
    revision: u64,
    variant_id: String,
    request_id: u64,
) -> HostResult<tauri::ipc::Response> {
    let shared = state.inner().clone();
    blocking(move || render_preview(&shared, &session_id, revision, &variant_id, request_id).map(tauri::ipc::Response::new)).await
}

#[tauri::command]
fn preview_metadata(state: State<'_, SharedHost>, session_id: String, request_id: u64) -> HostResult<PreviewMetadata> {
    let host = lock(state.inner())?;
    let record = host.record(&session_id)?;
    record
        .preview
        .as_ref()
        .filter(|m| {
            m.request_id == request_id && record.latest_request == Some(request_id) && m.document_revision == record.session.revision()
        })
        .cloned()
        .ok_or_else(|| {
            error(
                Code::RevisionConflict,
                "preview metadata is not current",
                "Request a new preview for the current revision.",
            )
        })
}

#[tauri::command]
async fn run_preflight(
    state: State<'_, SharedHost>,
    session_id: String,
    revision: u64,
    variant_id: String,
) -> HostResult<Vec<design_model::Issue>> {
    let shared = state.inner().clone();
    blocking(move || {
        let (doc, resources, _lease) = lock(&shared)?.snapshot(shared.clone(), &session_id, revision)?;
        match design_render::render_document(&doc, &variant_id, &resources, RenderOptions::default()) {
            Ok(out) => Ok(out.report.warnings),
            Err(issues) => Ok(issues),
        }
    })
    .await
}

#[tauri::command]
async fn open_project(app: tauri::AppHandle, state: State<'_, SharedHost>) -> HostResult<Option<SessionView>> {
    let shared = state.inner().clone();
    blocking(move || {
        let Some(selected) = app.dialog().file().add_filter("SILIQ project", &["siliqcg"]).blocking_pick_file() else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|_| {
            error(
                Code::PathDenied,
                "only local project files are supported",
                "Choose a local project file.",
            )
        })?;
        let grant = grants::open_local(&path, false).map_err(|i| vec![i])?;
        let workspace = lock(&shared)?.workspace.clone();
        let loaded = project_store::load_project_file(grant, &workspace).map_err(|i| vec![i])?;
        let view = lock(&shared)?.insert(loaded.document, loaded.resources, loaded.workspace, Some(path), loaded.issues)?;
        checkpoint(&shared, view).map(Some)
    })
    .await
}

#[tauri::command]
async fn save_project(
    app: tauri::AppHandle,
    state: State<'_, SharedHost>,
    session_id: String,
    expected_revision: u64,
    save_as: Option<bool>,
) -> HostResult<Option<project_store::SaveReceipt>> {
    let shared = state.inner().clone();
    blocking(move || {
        let previous = {
            let host = lock(&shared)?;
            let record = host.record(&session_id)?;
            record.check_revision(expected_revision)?;
            record.save_path.clone()
        };
        let (path, overwrite) = if let Some(path) = previous.filter(|_| !save_as.unwrap_or(false)) {
            (path, true)
        } else {
            let Some(selected) = app
                .dialog()
                .file()
                .add_filter("SILIQ project", &["siliqcg"])
                .set_file_name("design.siliqcg")
                .blocking_save_file()
            else {
                return Ok(None);
            };
            let path = selected.into_path().map_err(|_| {
                error(
                    Code::PathDenied,
                    "only local project destinations are supported",
                    "Choose a local file.",
                )
            })?;
            // The native Save dialog confirms replacement; no frontend path or
            // overwrite flag can authorize replacement of another file.
            let overwrite = path.symlink_metadata().is_ok();
            if overwrite {
                let _ = grants::open_local(&path, false).map_err(|i| vec![i])?;
            }
            (path, overwrite)
        };
        let (grant, name) = grants::save_grant(&path).map_err(|i| vec![i])?;
        let mut host = lock(&shared)?;
        let record = host.record_mut(&session_id)?;
        record.check_revision(expected_revision)?;
        let receipt = project_store::save_project_in(
            grant.directory(),
            std::ffi::OsStr::new(&name),
            record.session.document(),
            &record.resources,
            overwrite,
        )
        .map_err(|i| vec![i])?;
        record.save_path = Some(path);
        Ok(Some(receipt))
    })
    .await
}

#[tauri::command]
async fn recover_project(state: State<'_, SharedHost>) -> HostResult<SessionView> {
    let shared = state.inner().clone();
    blocking(move || {
        let (recovery, workspace) = {
            let host = lock(&shared)?;
            (host.recovery.clone(), host.workspace.clone())
        };
        let bytes = read_recovery_pointer(&recovery)?;
        let name = String::from_utf8(bytes)
            .map_err(|_| error(Code::SaveFailed, "invalid recovery workspace reference", "Open a saved project."))?;
        if name.is_empty()
            || name.len() > 512
            || !name
                .split('/')
                .all(|part| !part.is_empty() && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
        {
            return Err(error(
                Code::PathDenied,
                "invalid recovery workspace reference",
                "Open a saved project.",
            ));
        }
        let loaded = project_store::recover_project(&workspace.join(name)).map_err(|i| vec![i])?;
        // Keep every resumed session directly under the app workspace. Repeated
        // recovery must not accumulate increasingly nested path references.
        let destination = workspace.join(host::opaque_id("document"));
        std::fs::rename(&loaded.workspace, &destination).map_err(|_| {
            error(
                Code::SaveFailed,
                "cannot adopt the recovered local workspace",
                "Open a saved project.",
            )
        })?;
        let resources = ResourceStore::open(&destination).map_err(|i| vec![i])?;
        let view = lock(&shared)?.insert(loaded.document, resources, destination, None, loaded.issues)?;
        checkpoint(&shared, view)
    })
    .await
}

fn read_recovery_pointer(recovery: &Path) -> HostResult<Vec<u8>> {
    let grant = grants::DirectoryGrant::new(recovery.to_owned()).map_err(|i| vec![i])?;
    // Only a missing final directory entry means there has never been a
    // checkpoint. A dangling symlink still exists and must retain its rejection.
    if matches!(
        rustix::fs::statat(grant.directory(), "latest", rustix::fs::AtFlags::SYMLINK_NOFOLLOW),
        Err(rustix::io::Errno::NOENT)
    ) {
        return Err(error(
            Code::SaveFailed,
            "no recoverable document has been committed",
            "Open a saved project or create a document.",
        ));
    }
    grants::read_selected(&recovery.join("latest"), 512).map_err(|i| vec![i])
}

#[tauri::command]
// Native state/picker arguments accompany the established typed IPC payload.
#[allow(clippy::too_many_arguments)]
async fn import_asset(
    app: tauri::AppHandle,
    state: State<'_, SharedHost>,
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
    kind: String,
    replace_id: Option<String>,
    embed_confirmed: Option<bool>,
) -> HostResult<Option<SessionView>> {
    let shared = state.inner().clone();
    blocking(move || {
        let digest = sha256_hex(
            &serde_json::to_vec(&("import", expected_revision, &kind, &replace_id, embed_confirmed)).expect("import descriptor"),
        );
        {
            let host = lock(&shared)?;
            let record = host.record(&session_id)?;
            if let Some(view) = record.replay(&transaction_id, &digest)? {
                drop(host);
                return checkpoint(&shared, view).map(Some);
            }
            record.check_revision(expected_revision)?;
        }
        let picker = match kind.as_str() {
            "raster" => app.dialog().file().add_filter("Images", &["png", "jpg", "jpeg", "webp"]),
            "svg" => app.dialog().file().add_filter("Outlined SVG", &["svg"]),
            "font" => app.dialog().file().add_filter("Local fonts", &["otf", "ttf", "ttc"]),
            _ => return Err(error(Code::Schema, "unsupported import kind", "Use raster, svg or font.")),
        };
        let Some(selected) = picker.blocking_pick_file() else {
            return Ok(None);
        };
        let path = selected
            .into_path()
            .map_err(|_| error(Code::PathDenied, "only local inputs are supported", "Choose a local file."))?;
        let bytes = grants::read_selected(
            &path,
            if kind == "font" {
                MAX_FONT_BYTES
            } else if kind == "svg" {
                10 * 1024 * 1024
            } else {
                MAX_IMAGE_BYTES
            },
        )
        .map_err(|i| vec![i])?;
        let sha = sha256_hex(&bytes);
        let id = replace_id.unwrap_or_else(|| format!("{}-{}", if kind == "font" { "font" } else { "asset" }, &sha[..16]));
        let (sub, ext, command) = if kind == "font" {
            let ext = if bytes.starts_with(b"OTTO") {
                "otf"
            } else if bytes.starts_with(&[0, 1, 0, 0]) || bytes.starts_with(b"true") {
                "ttf"
            } else if bytes.starts_with(b"ttcf") {
                if bytes.len() < 12 || u32::from_be_bytes(bytes[8..12].try_into().unwrap()) > 16 {
                    return Err(error(
                        Code::ResourceLimit,
                        "font collection has too many faces or an invalid header",
                        "Use a font collection with at most 16 faces.",
                    ));
                }
                "ttc"
            } else {
                return Err(error(
                    Code::FontMissing,
                    "file content is not an OpenType or TrueType font",
                    "Choose a valid local font file.",
                ));
            };
            let font = Font {
                id: id.clone(),
                name_hint: path.file_stem().unwrap_or_default().to_string_lossy().chars().take(150).collect(),
                state: ResolveState::Resolved,
                sha256: Some(sha.clone()),
                face_index: 0,
                package_policy: if embed_confirmed.unwrap_or(false) {
                    PackagePolicy::EmbedConfirmed
                } else {
                    PackagePolicy::ReferenceOnly
                },
            };
            ("fonts", ext, Command::ImportFont { font })
        } else {
            let mime = if kind == "svg" {
                design_svg::import_svg(&bytes).map_err(|e| {
                    error(
                        match e.kind {
                            design_svg::SvgErrorKind::ExternalReference => Code::ExternalReference,
                            design_svg::SvgErrorKind::ResourceLimit => Code::ResourceLimit,
                            _ => Code::SvgUnsupported,
                        },
                        e.message,
                        "Choose an outlined SVG that uses only the supported local vector subset.",
                    )
                })?;
                MimeHint::Svg
            } else {
                match design_render::raster::sniff(&bytes) {
                    design_render::raster::Sniffed::Png => MimeHint::Png,
                    design_render::raster::Sniffed::Jpeg => MimeHint::Jpeg,
                    design_render::raster::Sniffed::Webp => MimeHint::Webp,
                    _ => {
                        return Err(error(
                            Code::AssetType,
                            "selected file content is not a supported still image",
                            "Choose PNG, JPEG or WebP.",
                        ));
                    }
                }
            };
            if kind == "raster" {
                design_render::raster::decode(&bytes, mime).map_err(|i| vec![i])?;
            }
            let asset = Asset {
                id: id.clone(),
                kind: if kind == "svg" { AssetKind::Vector } else { AssetKind::Raster },
                state: ResolveState::Resolved,
                sha256: Some(sha.clone()),
                mime_hint: mime,
            };
            ("assets", mime.extension(), Command::ImportAsset { asset })
        };
        let mut host = lock(&shared)?;
        let record = host.record_mut(&session_id)?;
        record.check_revision(expected_revision)?;
        let grant = grants::DirectoryGrant::new(record.workspace.join(sub)).map_err(|i| vec![i])?;
        let filename = format!("{sha}.{ext}");
        if !record.workspace.join(sub).join(&filename).exists() {
            grant.write_new(&filename, &bytes).map_err(|i| vec![i])?;
        }
        record.resources = ResourceStore::open(&record.workspace).map_err(|i| vec![i])?;
        if let Command::ImportFont { font } = &command {
            design_render::text::FontSet::load(&[font], &record.resources)?;
        }
        let view = mutate(&mut host, &session_id, expected_revision, &transaction_id, Mutation::Edit(command))?;
        host.record_mut(&session_id)?.remember(&transaction_id, digest, view.clone());
        drop(host);
        checkpoint(&shared, view).map(Some)
    })
    .await
}

#[tauri::command]
async fn export_png(
    app: tauri::AppHandle,
    state: State<'_, SharedHost>,
    session_id: String,
    revision: u64,
    variant_id: String,
) -> HostResult<Option<design_render::RenderReport>> {
    let shared = state.inner().clone();
    blocking(move || {
        let (doc, resources, _lease) = lock(&shared)?.snapshot(shared.clone(), &session_id, revision)?;
        let out = design_render::render_document(&doc, &variant_id, &resources, RenderOptions::default())?;
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("PNG image", &["png"])
            .set_file_name(format!("{variant_id}.png"))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|_| {
            error(
                Code::PathDenied,
                "only local export destinations are supported",
                "Choose a local file.",
            )
        })?;
        let (grant, name) = grants::save_grant(&path).map_err(|i| vec![i])?;
        grant.write_new(&name, &out.png).map_err(|i| vec![i])?;
        Ok(Some(out.report))
    })
    .await
}

fn font_extension(bytes: &[u8]) -> HostResult<&'static str> {
    if bytes.starts_with(b"OTTO") {
        Ok("otf")
    } else if bytes.starts_with(&[0, 1, 0, 0]) || bytes.starts_with(b"true") {
        Ok("ttf")
    } else if bytes.starts_with(b"ttcf") {
        if bytes.len() < 12 || !(1..=16).contains(&u32::from_be_bytes(bytes[8..12].try_into().unwrap())) {
            return Err(error(
                Code::ResourceLimit,
                "font collection must contain between one and sixteen faces",
                "Choose the exact declared font file with a valid collection header.",
            ));
        }
        Ok("ttc")
    } else {
        Err(error(
            Code::FontMissing,
            "selected content is not an OpenType or TrueType font",
            "Choose the exact declared local font file.",
        ))
    }
}

fn require_exact_font_bytes(font: &Font, bytes: &[u8]) -> HostResult<()> {
    if font.sha256.as_deref() != Some(sha256_hex(bytes).as_str()) {
        return Err(vec![
            design_model::Issue::error(
                Code::FontMissing,
                "selected font bytes do not match the declared font hash",
                "Select the exact original font file. To intentionally change the typeface, import a new font and choose it explicitly.",
            )
            .font(&font.id),
        ]);
    }
    Ok(())
}

#[tauri::command]
async fn resolve_font(
    app: tauri::AppHandle,
    state: State<'_, SharedHost>,
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
    font_id: String,
) -> HostResult<Option<SessionView>> {
    let shared = state.inner().clone();
    blocking(move || {
        let digest = sha256_hex(&serde_json::to_vec(&("resolve-font", expected_revision, &font_id)).expect("font resolution descriptor"));
        let mut font = {
            let host = lock(&shared)?;
            let record = host.record(&session_id)?;
            if let Some(view) = record.replay(&transaction_id, &digest)? {
                drop(host);
                return checkpoint(&shared, view).map(Some);
            }
            record.check_revision(expected_revision)?;
            let font = record
                .session
                .document()
                .fonts
                .iter()
                .find(|f| f.id == font_id)
                .cloned()
                .ok_or_else(|| {
                    error(
                        Code::FontMissing,
                        "unknown declared font ID",
                        "Choose a font declared by this document.",
                    )
                })?;
            if font.sha256.is_none() {
                return Err(error(
                    Code::FontMissing,
                    "this unresolved font has no known source hash",
                    "Import a new local font and choose it explicitly; a substitute cannot be resolved automatically.",
                ));
            }
            font
        };
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("Exact original local font", &["otf", "ttf", "ttc"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|_| {
            error(
                Code::PathDenied,
                "only local fonts can be resolved",
                "Choose a local original font file.",
            )
        })?;
        let bytes = grants::read_selected(&path, MAX_FONT_BYTES).map_err(|i| vec![i])?;
        require_exact_font_bytes(&font, &bytes)?;
        let extension = font_extension(&bytes)?;
        let mut host = lock(&shared)?;
        let record = host.record_mut(&session_id)?;
        record.check_revision(expected_revision)?;
        let name = format!("{}.{}", font.sha256.as_ref().unwrap(), extension);
        if !record.workspace.join("fonts").join(&name).exists() {
            grants::DirectoryGrant::new(record.workspace.join("fonts"))
                .and_then(|grant| grant.write_new(&name, &bytes))
                .map_err(|i| vec![i])?;
        }
        record.resources = ResourceStore::open(&record.workspace).map_err(|i| vec![i])?;
        font.state = ResolveState::Resolved; // Preserve hash, face, name and embedding permission.
        design_render::text::FontSet::load(&[&font], &record.resources)?;
        let view = mutate(
            &mut host,
            &session_id,
            expected_revision,
            &transaction_id,
            Mutation::Edit(Command::ImportFont { font }),
        )?;
        host.record_mut(&session_id)?.remember(&transaction_id, digest, view.clone());
        drop(host);
        checkpoint(&shared, view).map(Some)
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportReceipt {
    render: design_render::RenderReport,
    export: design_export::ExportReport,
}

#[tauri::command]
async fn export_design(
    app: tauri::AppHandle,
    state: State<'_, SharedHost>,
    session_id: String,
    revision: u64,
    variant_id: String,
    recipe: design_export::ExportRecipe,
) -> HostResult<Option<ExportReceipt>> {
    let shared = state.inner().clone();
    blocking(move || {
        let (doc, resources, _lease) = lock(&shared)?.snapshot(shared.clone(), &session_id, revision)?;
        let rendered = design_render::render_document(&doc, &variant_id, &resources, RenderOptions::default())?;
        let exported = design_export::export(&rendered, &recipe).map_err(|i| vec![i])?;
        let extension = recipe.format.extension();
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("Design export", &[extension])
            .set_file_name(format!("{variant_id}.{extension}"))
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|_| {
            error(
                Code::PathDenied,
                "only local export destinations are supported",
                "Choose a local file.",
            )
        })?;
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_none_or(|e| !e.eq_ignore_ascii_case(extension))
        {
            return Err(error(
                Code::ExportUnsupported,
                "file extension differs from the selected export format",
                "Use the extension shown in the native save dialog.",
            ));
        }
        let (grant, name) = grants::save_grant(&path).map_err(|i| vec![i])?;
        grant.write_new(&name, &exported.bytes).map_err(|i| vec![i])?;
        Ok(Some(ExportReceipt {
            render: rendered.report,
            export: exported.report,
        }))
    })
    .await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let workspace = app.path().app_local_data_dir()?.join("workspace");
            let fixtures = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/public-safe")
            } else {
                app.path().resource_dir()?.join("public-safe")
            };
            let host = Host::new(workspace, fixtures)
                .map_err(|i| std::io::Error::other(i.first().map(ToString::to_string).unwrap_or_default()))?;
            app.manage(Arc::new(Mutex::new(host)));
            app.manage(Arc::new(Mutex::new(batch::BatchHost::default())));

            tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
                .on_navigation(|url| {
                    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                        || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
                        || (cfg!(debug_assertions)
                            && matches!(url.scheme(), "http" | "ws")
                            && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
                            && url.port() == Some(1420))
                })
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_fixture,
            new_document,
            read_session,
            close_session,
            apply_edit,
            undo_edit,
            redo_edit,
            set_mode,
            request_preview,
            preview_metadata,
            run_preflight,
            open_project,
            save_project,
            recover_project,
            import_asset,
            resolve_font,
            export_png,
            export_design,
            batch::select_dataset,
            batch::preflight_batch,
            batch::request_batch_preview,
            batch::start_batch,
            batch::batch_status,
            batch::cancel_batch,
            batch::resume_batch,
            batch::list_saved_batches,
            batch::resume_saved_batch,
            storage::storage_status,
            storage::cleanup_storage
        ])
        .run(tauri::generate_context!())
        .expect("run the local SILIQ editor");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_pointer_only_missing_entry_is_classified_as_no_checkpoint() {
        let path = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(host::opaque_id("recovery-pointer-test"));
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            read_recovery_pointer(&path).unwrap_err()[0].message,
            "no recoverable document has been committed"
        );
        std::os::unix::fs::symlink("absent", path.join("latest")).unwrap();
        assert_eq!(
            read_recovery_pointer(&path).unwrap_err(),
            vec![grants::read_selected(&path.join("latest"), 512).unwrap_err()]
        );
        std::fs::remove_file(path.join("latest")).unwrap();
        std::fs::write(path.join("latest"), vec![b'x'; 513]).unwrap();
        assert_eq!(read_recovery_pointer(&path).unwrap_err()[0].code, Code::ResourceLimit);
        std::fs::remove_file(path.join("latest")).unwrap();
        std::fs::create_dir(path.join("latest")).unwrap();
        assert_eq!(read_recovery_pointer(&path).unwrap_err()[0].code, Code::PathDenied);
        assert_eq!(
            read_recovery_pointer(&path.join("absent-parent")).unwrap_err()[0].code,
            Code::PathDenied
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn font_resolution_refuses_other_bytes_before_any_document_mutation() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/public-safe");
        let doc = design_model::load_document(&std::fs::read(fixtures.join("documents/basic.json")).unwrap()).unwrap();
        let font = &doc.fonts[0];
        let store = ResourceStore::open(&fixtures.join("resources")).unwrap();
        let (bytes, _) = store
            .read("fonts", font.sha256.as_ref().unwrap(), &["otf", "ttf", "ttc"], MAX_FONT_BYTES)
            .unwrap();
        require_exact_font_bytes(font, &bytes).unwrap();
        assert_eq!(font_extension(&bytes).unwrap(), "otf");
        let rejected = require_exact_font_bytes(font, b"another font").unwrap_err();
        assert_eq!(rejected[0].code, Code::FontMissing);
        assert_eq!(rejected[0].location.font_id.as_deref(), Some(font.id.as_str()));
    }
    #[test]
    fn native_frame_uses_straight_alpha_without_double_premultiplication() {
        let path = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(host::opaque_id("native-alpha-test"));
        let mut host = Host::new(path.clone(), PathBuf::new()).unwrap();
        let workspace = host.create_workspace().unwrap();
        let doc = design_model::load_document(serde_json::to_string(&serde_json::json!({
            "format":"siliq-creative-generator-core","schemaVersion":design_model::SCHEMA_VERSION,"documentId":"alpha-fixture","revision":1,"title":"Transparent alpha fixture","colorSpace":"sRGB",
            "baseArtboard":{"width":64,"height":64,"rootNodeId":"root"},
            "nodes":[{"id":"root","name":"root","type":"group","bounds":{"x":0,"y":0,"width":64,"height":64},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"childIds":["red"]},
                     {"id":"red","name":"red","type":"rect","bounds":{"x":8,"y":8,"width":16,"height":16},"rotationDeg":0,"opacity":0.5,"visible":true,"locked":false,"fill":{"kind":"solid","color":"#FF0000"},"cornerRadius":0}],
            "fonts":[],"assets":[],"slots":[],"variants":[{"id":"square","label":"alpha","width":64,"height":64,"approvalStatus":"approved","nodePatches":[]}]
        })).unwrap().as_bytes()).unwrap();
        let view = host
            .insert(doc, ResourceStore::open(&workspace).unwrap(), workspace, None, Vec::new())
            .unwrap();
        let shared = Arc::new(Mutex::new(host));
        let bytes = render_preview(&shared, &view.session_id, 1, "square", 1).unwrap();
        assert_eq!(&bytes[..4], &[0, 0, 0, 0]);
        let offset = (12 * 64 + 12) * 4;
        assert_eq!(&bytes[offset..offset + 4], &[255, 0, 0, 128]);
        assert_eq!(
            lock(&shared)
                .unwrap()
                .record(&view.session_id)
                .unwrap()
                .preview
                .as_ref()
                .unwrap()
                .alpha_type,
            "straight"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn binary_preview_matches_export_and_rejects_stale_requests() {
        let path = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(host::opaque_id("native-preview-test"));
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/public-safe");
        let shared = Arc::new(Mutex::new(Host::new(path.clone(), fixtures).unwrap()));
        let view = lock(&shared).unwrap().open_fixture("basic").unwrap();
        let bytes = render_preview(&shared, &view.session_id, 1, "square", 1).unwrap();
        let host = lock(&shared).unwrap();
        let record = host.record(&view.session_id).unwrap();
        let metadata = record.preview.as_ref().unwrap();
        assert_eq!(metadata.payload_length, bytes.len());
        assert_eq!(metadata.frame_sha256, sha256_hex(&bytes));
        assert_eq!(metadata.pixel_format, "RGBA8");
        assert_eq!(metadata.stride, metadata.width * 4);
        assert_eq!(metadata.payload_length, metadata.stride as usize * metadata.height as usize);
        assert_eq!((metadata.width, metadata.height), (1080, 1080));
        let exported =
            design_render::render_document(record.session.document(), "square", &record.resources, RenderOptions::default()).unwrap();
        assert_eq!(metadata.output_sha256, sha256_hex(&exported.png));
        assert_eq!(bytes, exported.rgba);
        drop(host);
        assert!(render_preview(&shared, &view.session_id, 1, "square", 1).is_err());
        mutate(
            &mut lock(&shared).unwrap(),
            &view.session_id,
            1,
            "edit-1",
            Mutation::Edit(Command::SetOpacity {
                node_id: "photo".into(),
                opacity: 0.6,
            }),
        )
        .unwrap();
        assert!(render_preview(&shared, &view.session_id, 1, "square", 2).is_err());
        let current = render_preview(&shared, &view.session_id, 2, "square", 2).unwrap();
        assert_ne!(bytes, current);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn recovery_checkpoint_retry_is_idempotent() {
        let path = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(host::opaque_id("native-recovery-test"));
        let shared = Arc::new(Mutex::new(Host::new(path.clone(), PathBuf::new()).unwrap()));
        let view = lock(&shared).unwrap().new_document().unwrap();
        checkpoint(&shared, view.clone()).unwrap();
        checkpoint(&shared, view).unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}
