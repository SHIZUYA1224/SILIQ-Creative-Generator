//! Native SwiftUI broker. Only typed JSON commands reach the canonical Rust
//! session. The caller holds its OS-selected security scope while passing paths.
use crate::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

pub const MAX_REQUEST_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

pub struct NativeBroker {
    shared: SharedHost,
    batches: batch::SharedBatches,
}

impl Drop for NativeBroker {
    fn drop(&mut self) {
        batch::shutdown_plain(&self.shared, &self.batches);
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    command: String,
    args: Value,
}

// Validate every object before parsing through Value, which would otherwise
// silently replace duplicate keys. Serde's recursion bound remains enabled.
struct UniqueJson;
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(self, mut m: M) -> Result<UniqueJson, M::Error> {
                let mut seen = std::collections::BTreeSet::new();
                while let Some(key) = m.next_key::<String>()? {
                    if !seen.insert(key) {
                        return Err(serde::de::Error::custom("duplicate native JSON key"));
                    }
                    if seen.len() > 100_000 {
                        return Err(serde::de::Error::custom("native JSON object exceeds its field limit"));
                    }
                    m.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(self, mut s: S) -> Result<UniqueJson, S::Error> {
                let mut count = 0;
                while s.next_element::<UniqueJson>()?.is_some() {
                    count += 1;
                    if count > 200_000 {
                        return Err(serde::de::Error::custom("native JSON array exceeds its item limit"));
                    }
                }
                Ok(UniqueJson)
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E: serde::de::Error>(self, _: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
        }
        d.deserialize_any(Visitor)
    }
}

fn parse_request(bytes: &[u8]) -> HostResult<Request> {
    let mut d = serde_json::Deserializer::from_slice(bytes);
    UniqueJson::deserialize(&mut d).and_then(|_| d.end()).map_err(|e| {
        error(
            Code::Schema,
            format!("invalid native JSON: {e}"),
            "Use unique fields and bounded JSON containers.",
        )
    })?;
    serde_json::from_slice(bytes).map_err(|e| {
        error(
            Code::Schema,
            format!("invalid native request: {e}"),
            "Use command and typed args only.",
        )
    })
}

macro_rules! args {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct $name { $($field : $ty),* }
    };
}
args!(Empty {});
args!(Fixture { fixture: Option<String> });
args!(SessionArg { session_id: String });
args!(Edit {
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
    command: Command
});
args!(Transaction {
    session_id: String,
    expected_revision: u64,
    transaction_id: String
});
args!(ModeArg {
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
    mode: Mode
});
args!(Preview {
    session_id: String,
    revision: u64,
    variant_id: String,
    request_id: u64
});
args!(Preflight {
    session_id: String,
    revision: u64,
    variant_id: String
});
args!(Open { path: PathBuf });
args!(Save {
    session_id: String,
    expected_revision: u64,
    path: PathBuf,
    overwrite: bool
});
args!(Import { session_id: String, expected_revision: u64, transaction_id: String, kind: String, replace_id: Option<String>, embed_confirmed: Option<bool>, path: PathBuf });
args!(ResolveFont {
    session_id: String,
    expected_revision: u64,
    transaction_id: String,
    font_id: String,
    path: PathBuf
});
args!(Export {
    session_id: String,
    revision: u64,
    variant_id: String,
    recipe: design_export::ExportRecipe,
    path: PathBuf
});
args!(Dataset {
    session_id: String,
    path: PathBuf
});
args!(PlanArg { plan_id: String });
args!(JobArg { job_id: String });
args!(BatchPreview {
    plan_id: String,
    item_index: usize
});
args!(ResumeSaved {
    snapshot_id: String,
    output_directory: PathBuf
});
args!(Cleanup {
    kind: storage::CleanupKind
});

fn decode<T: DeserializeOwned>(value: Value) -> HostResult<T> {
    serde_json::from_value(value).map_err(|e| {
        error(
            Code::Schema,
            format!("invalid native arguments: {e}"),
            "Use the documented typed command fields.",
        )
    })
}
fn value<T: serde::Serialize>(result: HostResult<T>) -> HostResult<Value> {
    result.and_then(|v| {
        serde_json::to_value(v).map_err(|_| error(Code::RenderFailed, "native response serialization failed", "Retry the operation."))
    })
}

fn imported_resource_result(view: SessionView, id: &str) -> HostResult<Value> {
    let mut result = value(Ok(view))?;
    result
        .as_object_mut()
        .expect("SessionView serializes as an object")
        .insert("importedResourceId".into(), json!(id));
    Ok(result)
}

pub fn error_response(issues: Vec<design_model::Issue>) -> Vec<u8> {
    serde_json::to_vec(&json!({"ok":false,"issues":issues})).expect("serializable native issues")
}

impl NativeBroker {
    pub fn new(workspace: PathBuf, fixtures: PathBuf) -> HostResult<Self> {
        Ok(Self {
            shared: Arc::new(Mutex::new(Host::new(workspace, fixtures)?)),
            batches: Arc::new(Mutex::new(batch::BatchHost::default())),
        })
    }

    pub fn request(&self, bytes: &[u8]) -> Vec<u8> {
        let result = if bytes.is_empty() || bytes.len() > MAX_REQUEST_BYTES {
            Err(error(
                Code::ResourceLimit,
                "native request must contain at most 20 MiB",
                "Split large requests or reduce the document.",
            ))
        } else {
            parse_request(bytes).and_then(|r| self.dispatch(&r.command, r.args))
        };
        let output = match result {
            Ok(v) => serde_json::to_vec(&json!({"ok":true,"value":v})).expect("serialized response"),
            Err(i) => error_response(i),
        };
        if output.len() > MAX_RESPONSE_BYTES {
            error_response(error(
                Code::ResourceLimit,
                "native response exceeds 64 MiB",
                "Use a smaller artboard or document.",
            ))
        } else {
            output
        }
    }

    fn dispatch(&self, command: &str, args: Value) -> HostResult<Value> {
        match command {
            "open_fixture" => {
                let a: Fixture = decode(args)?;
                let view = lock(&self.shared)?.open_fixture(a.fixture.as_deref().unwrap_or("retail-promo-template"))?;
                value(checkpoint(&self.shared, view))
            }
            "new_document" => {
                let _: Empty = decode(args)?;
                let view = lock(&self.shared)?.new_document()?;
                value(checkpoint(&self.shared, view))
            }
            "read_session" => {
                let a: SessionArg = decode(args)?;
                value(Ok(lock(&self.shared)?.record(&a.session_id)?.view(&a.session_id)))
            }
            "close_session" => {
                let a: SessionArg = decode(args)?;
                let mut host = lock(&self.shared)?;
                host.record(&a.session_id)?;
                host.sessions.remove(&a.session_id);
                Ok(Value::Null)
            }
            "apply_edit" => {
                let a: Edit = decode(args)?;
                let view = mutate(
                    &mut *lock(&self.shared)?,
                    &a.session_id,
                    a.expected_revision,
                    &a.transaction_id,
                    Mutation::Edit(a.command),
                )?;
                value(checkpoint(&self.shared, view))
            }
            "undo_edit" | "redo_edit" => {
                let a: Transaction = decode(args)?;
                let op = if command == "undo_edit" { Mutation::Undo } else { Mutation::Redo };
                let view = mutate(&mut *lock(&self.shared)?, &a.session_id, a.expected_revision, &a.transaction_id, op)?;
                value(checkpoint(&self.shared, view))
            }
            "set_mode" => {
                let a: ModeArg = decode(args)?;
                let view = mutate(
                    &mut *lock(&self.shared)?,
                    &a.session_id,
                    a.expected_revision,
                    &a.transaction_id,
                    Mutation::Mode(a.mode),
                )?;
                value(checkpoint(&self.shared, view))
            }
            "native_preview" => {
                let a: Preview = decode(args)?;
                let out = render_preview_output(&self.shared, &a.session_id, a.revision, &a.variant_id, a.request_id)?;
                if out.png.len() > MAX_RESPONSE_BYTES / 2 {
                    return Err(error(
                        Code::ResourceLimit,
                        "PNG preview exceeds the bridge limit",
                        "Use a smaller artboard.",
                    ));
                }
                let mut metadata = {
                    let host = lock(&self.shared)?;
                    let record = host.record(&a.session_id)?;
                    record.check_revision(a.revision)?;
                    record
                        .preview
                        .as_ref()
                        .filter(|m| m.request_id == a.request_id && record.latest_request == Some(a.request_id))
                        .cloned()
                        .ok_or_else(|| {
                            error(
                                Code::RevisionConflict,
                                "native preview was superseded",
                                "Request the current session revision.",
                            )
                        })?
                };
                metadata.pixel_format = "PNG";
                metadata.stride = 0;
                metadata.payload_length = out.png.len();
                metadata.frame_sha256 = metadata.output_sha256.clone();
                Ok(json!({"pngBase64":STANDARD.encode(out.png),"metadata":metadata}))
            }
            "run_preflight" => {
                let a: Preflight = decode(args)?;
                let (doc, resources, _lease) = lock(&self.shared)?.snapshot(self.shared.clone(), &a.session_id, a.revision)?;
                value(Ok(
                    match design_render::render_document(&doc, &a.variant_id, &resources, RenderOptions::default()) {
                        Ok(out) => out.report.warnings,
                        Err(i) => i,
                    },
                ))
            }
            "open_project_path" => {
                let a: Open = decode(args)?;
                value(self.open_project(a.path))
            }
            "save_project_path" => {
                let a: Save = decode(args)?;
                value(self.save_project(a))
            }
            "recover_project" => {
                let _: Empty = decode(args)?;
                value(self.recover())
            }
            "import_asset_path" => {
                let a: Import = decode(args)?;
                self.import(a)
            }
            "resolve_font_path" => {
                let a: ResolveFont = decode(args)?;
                value(self.resolve_font(a))
            }
            "export_design_path" => {
                let a: Export = decode(args)?;
                value(self.export(a))
            }
            "select_dataset_path" => {
                let a: Dataset = decode(args)?;
                value(batch::select_dataset_path(&self.shared, &self.batches, a.session_id, a.path))
            }
            "preflight_batch_path" => value(batch::preflight_batch_path(&self.shared, &self.batches, decode(args)?)),
            "resume_saved_batch_path" => {
                let a: ResumeSaved = decode(args)?;
                value(batch::resume_saved_batch_path(
                    &self.shared,
                    &self.batches,
                    a.snapshot_id,
                    a.output_directory,
                ))
            }
            "list_saved_batches" => {
                let _: Empty = decode(args)?;
                value(batch::list_saved_batches_plain(&self.shared))
            }
            "start_batch" => {
                let a: PlanArg = decode(args)?;
                value(batch::start_batch_plain(&self.batches, a.plan_id))
            }
            "batch_status" => {
                let a: JobArg = decode(args)?;
                value(batch::batch_status_plain(&self.batches, a.job_id))
            }
            "cancel_batch" => {
                let a: JobArg = decode(args)?;
                value(batch::cancel_batch_plain(&self.batches, a.job_id))
            }
            "resume_batch" => {
                let a: JobArg = decode(args)?;
                value(batch::resume_batch_plain(&self.batches, a.job_id))
            }
            "native_batch_preview" => {
                let a: BatchPreview = decode(args)?;
                let png = batch::batch_preview_plain(&self.shared, &self.batches, a.plan_id, a.item_index)?;
                Ok(json!({"pngBase64":STANDARD.encode(png)}))
            }
            "storage_status" => {
                let _: Empty = decode(args)?;
                value(storage::storage_status_plain(&self.shared, &self.batches))
            }
            "cleanup_storage" => {
                let a: Cleanup = decode(args)?;
                value(storage::cleanup_storage_plain(&self.shared, &self.batches, a.kind))
            }
            _ => Err(error(
                Code::EditForbidden,
                "unknown native command",
                "Use only the documented command allowlist.",
            )),
        }
    }

    fn open_project(&self, path: PathBuf) -> HostResult<SessionView> {
        let file = grants::open_local(&path, false).map_err(|i| vec![i])?;
        let workspace = lock(&self.shared)?.workspace.clone();
        let loaded = project_store::load_project_file(file, &workspace).map_err(|i| vec![i])?;
        let view = lock(&self.shared)?.insert(loaded.document, loaded.resources, loaded.workspace, Some(path), loaded.issues)?;
        checkpoint(&self.shared, view)
    }

    fn save_project(&self, a: Save) -> HostResult<project_store::SaveReceipt> {
        if a.overwrite {
            let _ = grants::open_local(&a.path, false).map_err(|i| vec![i])?;
        }
        let (grant, name) = grants::save_grant(&a.path).map_err(|i| vec![i])?;
        let mut host = lock(&self.shared)?;
        let record = host.record_mut(&a.session_id)?;
        record.check_revision(a.expected_revision)?;
        let receipt = project_store::save_project_in(
            grant.directory(),
            std::ffi::OsStr::new(&name),
            record.session.document(),
            &record.resources,
            a.overwrite,
        )
        .map_err(|i| vec![i])?;
        record.save_path = Some(a.path);
        Ok(receipt)
    }

    fn recover(&self) -> HostResult<SessionView> {
        let (recovery, workspace) = {
            let host = lock(&self.shared)?;
            (host.recovery.clone(), host.workspace.clone())
        };
        let name = String::from_utf8(read_recovery_pointer(&recovery)?)
            .map_err(|_| error(Code::SaveFailed, "invalid recovery workspace reference", "Open a saved project."))?;
        if name.is_empty()
            || !name
                .split('/')
                .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
        {
            return Err(error(
                Code::PathDenied,
                "invalid recovery workspace reference",
                "Open a saved project.",
            ));
        }
        let loaded = project_store::recover_project(&workspace.join(name)).map_err(|i| vec![i])?;
        let destination = workspace.join(host::opaque_id("document"));
        std::fs::rename(loaded.workspace, &destination)
            .map_err(|_| error(Code::SaveFailed, "cannot adopt recovery workspace", "Open a saved project."))?;
        let resources = ResourceStore::open(&destination).map_err(|i| vec![i])?;
        let view = lock(&self.shared)?.insert(loaded.document, resources, destination, None, loaded.issues)?;
        checkpoint(&self.shared, view)
    }

    fn import(&self, a: Import) -> HostResult<Value> {
        let digest = sha256_hex(
            &serde_json::to_vec(&(
                "import-path",
                a.expected_revision,
                &a.kind,
                &a.replace_id,
                a.embed_confirmed,
                &a.path,
            ))
            .expect("import descriptor"),
        );
        {
            let host = lock(&self.shared)?;
            let record = host.record(&a.session_id)?;
            if let Some(v) = record.replay(&a.transaction_id, &digest)? {
                let id = record
                    .imported_resource(&a.transaction_id)
                    .ok_or_else(|| {
                        error(
                            Code::RevisionConflict,
                            "native import result metadata is unavailable",
                            "Read the current session and choose the registered resource explicitly.",
                        )
                    })?
                    .to_owned();
                drop(host);
                return imported_resource_result(checkpoint(&self.shared, v)?, &id);
            }
            record.check_revision(a.expected_revision)?;
        }
        if !matches!(a.kind.as_str(), "font" | "raster" | "svg") {
            return Err(error(Code::Schema, "unsupported import kind", "Use raster, svg or font."));
        }
        let bytes = grants::read_selected(
            &a.path,
            if a.kind == "font" {
                MAX_FONT_BYTES
            } else if a.kind == "svg" {
                10 * 1024 * 1024
            } else {
                MAX_IMAGE_BYTES
            },
        )
        .map_err(|i| vec![i])?;
        let sha = sha256_hex(&bytes);
        let id = a
            .replace_id
            .unwrap_or_else(|| format!("{}-{}", if a.kind == "font" { "font" } else { "asset" }, &sha[..16]));
        let imported_id = id.clone();
        let (sub, ext, command) = if a.kind == "font" {
            let ext = font_extension(&bytes)?;
            let font = Font {
                id,
                name_hint: a.path.file_stem().unwrap_or_default().to_string_lossy().chars().take(150).collect(),
                state: ResolveState::Resolved,
                sha256: Some(sha.clone()),
                face_index: 0,
                package_policy: if a.embed_confirmed.unwrap_or(false) {
                    PackagePolicy::EmbedConfirmed
                } else {
                    PackagePolicy::ReferenceOnly
                },
            };
            ("fonts", ext, Command::ImportFont { font })
        } else {
            let mime = if a.kind == "svg" {
                design_svg::import_svg(&bytes).map_err(|e| {
                    error(
                        match e.kind {
                            design_svg::SvgErrorKind::ExternalReference => Code::ExternalReference,
                            design_svg::SvgErrorKind::ResourceLimit => Code::ResourceLimit,
                            _ => Code::SvgUnsupported,
                        },
                        e.message,
                        "Use a supported outlined local SVG.",
                    )
                })?;
                MimeHint::Svg
            } else {
                match design_render::raster::sniff(&bytes) {
                    design_render::raster::Sniffed::Png => MimeHint::Png,
                    design_render::raster::Sniffed::Jpeg => MimeHint::Jpeg,
                    design_render::raster::Sniffed::Webp => MimeHint::Webp,
                    _ => return Err(error(Code::AssetType, "unsupported still-image content", "Use PNG, JPEG or WebP.")),
                }
            };
            if a.kind == "raster" {
                design_render::raster::decode(&bytes, mime).map_err(|i| vec![i])?;
            }
            (
                "assets",
                mime.extension(),
                Command::ImportAsset {
                    asset: Asset {
                        id,
                        kind: if a.kind == "svg" { AssetKind::Vector } else { AssetKind::Raster },
                        state: ResolveState::Resolved,
                        sha256: Some(sha.clone()),
                        mime_hint: mime,
                    },
                },
            )
        };
        let mut host = lock(&self.shared)?;
        let record = host.record_mut(&a.session_id)?;
        record.check_revision(a.expected_revision)?;
        let grant = grants::DirectoryGrant::new(record.workspace.join(sub)).map_err(|i| vec![i])?;
        let name = format!("{sha}.{ext}");
        if !record.workspace.join(sub).join(&name).exists() {
            grant.write_new(&name, &bytes).map_err(|i| vec![i])?;
        }
        record.resources = ResourceStore::open(&record.workspace).map_err(|i| vec![i])?;
        if let Command::ImportFont { font } = &command {
            design_render::text::FontSet::load(&[font], &record.resources)?;
        }
        let view = mutate(
            &mut host,
            &a.session_id,
            a.expected_revision,
            &a.transaction_id,
            Mutation::Edit(command),
        )?;
        host.record_mut(&a.session_id)?.remember(&a.transaction_id, digest, view.clone());
        host.record_mut(&a.session_id)?
            .remember_imported_resource(&a.transaction_id, imported_id.clone());
        drop(host);
        imported_resource_result(checkpoint(&self.shared, view)?, &imported_id)
    }

    fn resolve_font(&self, a: ResolveFont) -> HostResult<SessionView> {
        let digest =
            sha256_hex(&serde_json::to_vec(&("resolve-font-path", a.expected_revision, &a.font_id, &a.path)).expect("font descriptor"));
        let mut font = {
            let host = lock(&self.shared)?;
            let record = host.record(&a.session_id)?;
            if let Some(v) = record.replay(&a.transaction_id, &digest)? {
                drop(host);
                return checkpoint(&self.shared, v);
            }
            record.check_revision(a.expected_revision)?;
            record
                .session
                .document()
                .fonts
                .iter()
                .find(|f| f.id == a.font_id)
                .cloned()
                .ok_or_else(|| error(Code::FontMissing, "unknown declared font", "Choose a declared font."))?
        };
        let bytes = grants::read_selected(&a.path, MAX_FONT_BYTES).map_err(|i| vec![i])?;
        require_exact_font_bytes(&font, &bytes)?;
        let ext = font_extension(&bytes)?;
        let mut host = lock(&self.shared)?;
        let record = host.record_mut(&a.session_id)?;
        record.check_revision(a.expected_revision)?;
        let name = format!("{}.{}", font.sha256.as_ref().unwrap(), ext);
        if !record.workspace.join("fonts").join(&name).exists() {
            grants::DirectoryGrant::new(record.workspace.join("fonts"))
                .and_then(|g| g.write_new(&name, &bytes))
                .map_err(|i| vec![i])?;
        }
        record.resources = ResourceStore::open(&record.workspace).map_err(|i| vec![i])?;
        font.state = ResolveState::Resolved;
        design_render::text::FontSet::load(&[&font], &record.resources)?;
        let view = mutate(
            &mut host,
            &a.session_id,
            a.expected_revision,
            &a.transaction_id,
            Mutation::Edit(Command::ImportFont { font }),
        )?;
        host.record_mut(&a.session_id)?.remember(&a.transaction_id, digest, view.clone());
        drop(host);
        checkpoint(&self.shared, view)
    }

    fn export(&self, a: Export) -> HostResult<Value> {
        let (doc, resources, _lease) = lock(&self.shared)?.snapshot(self.shared.clone(), &a.session_id, a.revision)?;
        let rendered = design_render::render_document(&doc, &a.variant_id, &resources, RenderOptions::default())?;
        let out = design_export::export(&rendered, &a.recipe).map_err(|i| vec![i])?;
        let (grant, name) = grants::save_grant(&a.path).map_err(|i| vec![i])?;
        if !name.to_ascii_lowercase().ends_with(&format!(".{}", a.recipe.format.extension())) {
            return Err(error(
                Code::ExportUnsupported,
                "export extension differs from its selected format",
                "Use the extension shown by the save dialog.",
            ));
        }
        grant.write_new(&name, &out.bytes).map_err(|i| vec![i])?;
        Ok(json!({"render":rendered.report,"export":out.report}))
    }
}
