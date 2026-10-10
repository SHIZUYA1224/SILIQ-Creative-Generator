use design_edit::{Command, Mode, Session};
use design_model::{Code, DesignDocument, Issue, load_document};
use design_render::resources::{ResourceStore, sha256_hex};
use serde::Serialize;
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub type HostResult<T> = Result<T, Vec<Issue>>;
pub type SharedHost = Arc<Mutex<Host>>;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn opaque_id(prefix: &str) -> String {
    let nonce = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let bytes = format!("{prefix}:{}:{nonce}:{:?}", std::process::id(), std::time::SystemTime::now());
    format!("{prefix}-{}", &sha256_hex(bytes.as_bytes())[..24])
}

pub fn error(code: Code, message: impl Into<String>, remedy: &str) -> Vec<Issue> {
    vec![Issue::error(code, message, remedy)]
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub session_id: String,
    pub document: DesignDocument,
    pub mode: Mode,
    pub can_undo: bool,
    pub can_redo: bool,
    pub issues: Vec<Issue>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMetadata {
    pub document_revision: u64,
    pub request_id: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub payload_length: usize,
    pub pixel_format: &'static str,
    pub color_space: &'static str,
    pub alpha_type: &'static str,
    pub quality: &'static str,
    pub output_sha256: String,
    pub frame_sha256: String,
    pub issues: Vec<Issue>,
}

pub struct SessionRecord {
    pub session: Session,
    pub resources: ResourceStore,
    pub workspace: PathBuf,
    pub save_path: Option<PathBuf>,
    pub issues: Vec<Issue>,
    pub latest_request: Option<u64>,
    pub preview: Option<PreviewMetadata>,
    pub checkpoint_revision: Option<u64>,
    transactions: BTreeMap<String, (String, SessionView, Option<String>)>,
    transaction_order: VecDeque<String>,
}

impl SessionRecord {
    pub fn view(&self, id: &str) -> SessionView {
        SessionView {
            session_id: id.to_owned(),
            document: self.session.document().clone(),
            mode: self.session.mode(),
            can_undo: self.session.can_undo(),
            can_redo: self.session.can_redo(),
            issues: self.issues.clone(),
        }
    }

    pub fn check_revision(&self, revision: u64) -> HostResult<()> {
        if self.session.revision() != revision {
            return Err(error(
                Code::RevisionConflict,
                "document revision has changed",
                "Reload the current document and repeat the operation.",
            ));
        }
        Ok(())
    }

    pub fn replay(&self, id: &str, digest: &str) -> HostResult<Option<SessionView>> {
        if id.is_empty() || id.len() > 128 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')) {
            return Err(error(
                Code::Schema,
                "transactionId must be 1–128 ASCII letters, numbers, - or _",
                "Use a fresh transaction ID for each committed operation.",
            ));
        }
        match self.transactions.get(id) {
            Some((previous, view, _)) if previous == digest => Ok(Some(view.clone())),
            Some(_) => Err(error(
                Code::RevisionConflict,
                "transactionId was reused for a different operation",
                "Use a fresh transaction ID.",
            )),
            None => Ok(None),
        }
    }

    pub fn remember(&mut self, id: &str, digest: String, view: SessionView) {
        if self.transactions.insert(id.to_owned(), (digest, view, None)).is_none() {
            self.transaction_order.push_back(id.to_owned());
        }
        if self.transaction_order.len() > 512
            && let Some(expired) = self.transaction_order.pop_front()
        {
            self.transactions.remove(&expired);
        }
        self.preview = None;
    }

    pub fn remember_imported_resource(&mut self, transaction_id: &str, resource_id: String) {
        if let Some((_, _, imported)) = self.transactions.get_mut(transaction_id) {
            *imported = Some(resource_id);
        }
    }

    pub fn imported_resource(&self, transaction_id: &str) -> Option<&str> {
        self.transactions
            .get(transaction_id)
            .and_then(|(_, _, imported)| imported.as_deref())
    }
}

pub struct Host {
    pub sessions: BTreeMap<String, SessionRecord>,
    pub workspace: PathBuf,
    pub fixtures: PathBuf,
    pub recovery: PathBuf,
    _workspace_lock: std::fs::File,
    pub in_flight: BTreeMap<String, PathBuf>,
}

impl Host {
    pub fn new(workspace: PathBuf, fixtures: PathBuf) -> HostResult<Self> {
        std::fs::create_dir_all(&workspace).map_err(|_| {
            error(
                Code::PathDenied,
                "cannot create the local app workspace",
                "Check local disk permissions and free space.",
            )
        })?;
        let directory = crate::grants::open_local(&workspace, true).map_err(|i| vec![i])?;
        let workspace_lock: std::fs::File = rustix::fs::openat(
            &directory,
            "app.lock",
            rustix::fs::OFlags::RDWR | rustix::fs::OFlags::CREATE | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .map_err(|_| {
            error(
                Code::PathDenied,
                "cannot reserve the app-owned workspace",
                "Check local workspace permissions.",
            )
        })?
        .into();
        let stat = rustix::fs::fstat(&workspace_lock).map_err(|_| {
            error(
                Code::PathDenied,
                "cannot inspect the app workspace lock",
                "Check local workspace permissions.",
            )
        })?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile || stat.st_nlink != 1 {
            return Err(error(
                Code::PathDenied,
                "unsafe app workspace lock",
                "Preserve the app workspace and inspect its permissions.",
            ));
        }
        rustix::fs::flock(&workspace_lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(|_| {
            error(
                Code::EditForbidden,
                "this local workspace is already open in another app process",
                "Use the existing app window or close it before starting another instance.",
            )
        })?;
        let recovery = workspace.join("recovery");
        std::fs::create_dir_all(&recovery).map_err(|_| {
            error(
                Code::PathDenied,
                "cannot create local recovery directory",
                "Check local disk permissions.",
            )
        })?;
        Ok(Self {
            sessions: BTreeMap::new(),
            workspace,
            fixtures,
            recovery,
            _workspace_lock: workspace_lock,
            in_flight: BTreeMap::new(),
        })
    }

    pub fn pin_workspace(&mut self, shared: SharedHost, path: PathBuf) -> WorkspaceLease {
        let id = opaque_id("operation");
        self.in_flight.insert(id.clone(), path);
        WorkspaceLease { shared, id }
    }

    pub fn snapshot(
        &mut self,
        shared: SharedHost,
        session_id: &str,
        revision: u64,
    ) -> HostResult<(DesignDocument, ResourceStore, WorkspaceLease)> {
        let record = self.record(session_id)?;
        record.check_revision(revision)?;
        let doc = record.session.document().clone();
        let resources = record.resources.clone();
        let path = record.workspace.clone();
        let lease = self.pin_workspace(shared, path);
        Ok((doc, resources, lease))
    }

    pub fn record(&self, id: &str) -> HostResult<&SessionRecord> {
        self.sessions
            .get(id)
            .ok_or_else(|| error(Code::PathDenied, "unknown or closed document session", "Open a document first."))
    }

    pub fn record_mut(&mut self, id: &str) -> HostResult<&mut SessionRecord> {
        self.sessions
            .get_mut(id)
            .ok_or_else(|| error(Code::PathDenied, "unknown or closed document session", "Open a document first."))
    }

    pub fn insert(
        &mut self,
        doc: DesignDocument,
        resources: ResourceStore,
        workspace: PathBuf,
        save_path: Option<PathBuf>,
        issues: Vec<Issue>,
    ) -> HostResult<SessionView> {
        let id = opaque_id("session");
        let record = SessionRecord {
            session: Session::new(doc, Mode::Authoring)?,
            resources,
            workspace,
            save_path,
            issues,
            latest_request: None,
            preview: None,
            checkpoint_revision: None,
            transactions: BTreeMap::new(),
            transaction_order: VecDeque::new(),
        };
        let view = record.view(&id);
        self.sessions.insert(id, record);
        Ok(view)
    }

    pub fn open_fixture(&mut self, fixture: &str) -> HostResult<SessionView> {
        if !matches!(
            fixture,
            "retail-promo-template" | "retail-promo" | "showcase" | "basic" | "blog-summary" | "blog-comparison"
        ) {
            return Err(error(Code::Schema, "unknown bundled fixture", "Choose a bundled example."));
        }
        let source = self.fixtures.join("documents").join(format!("{fixture}.json"));
        let bytes = std::fs::read(source).map_err(|_| {
            error(
                Code::AssetMissing,
                "the bundled example document is missing",
                "Reinstall the complete local app bundle.",
            )
        })?;
        let doc = load_document(&bytes)?;
        let source_store = ResourceStore::open(&self.fixtures.join("resources")).map_err(|i| vec![i])?;
        let workspace = self.create_workspace()?;
        copy_resources(&doc, &source_store, &workspace)?;
        self.insert(
            doc,
            ResourceStore::open(&workspace).map_err(|i| vec![i])?,
            workspace,
            None,
            Vec::new(),
        )
    }

    pub fn create_workspace(&self) -> HostResult<PathBuf> {
        let path = self.workspace.join(opaque_id("document"));
        std::fs::create_dir(&path)
            .and_then(|_| std::fs::create_dir(path.join("assets")))
            .and_then(|_| std::fs::create_dir(path.join("fonts")))
            .map_err(|_| {
                error(
                    Code::PathDenied,
                    "cannot create a private document workspace",
                    "Check local disk permissions.",
                )
            })?;
        Ok(path)
    }

    pub fn new_document(&mut self) -> HostResult<SessionView> {
        let mut doc = load_document(serde_json::to_string(&serde_json::json!({
            "format":"siliq-creative-generator-core", "schemaVersion": design_model::SCHEMA_VERSION, "documentId":opaque_id("design"), "revision":1,
            "title":"新しいデザイン", "colorSpace":"sRGB", "baseArtboard":{"width":1080,"height":1080,"rootNodeId":"root"},
            "nodes":[{"id":"root","name":"アートボード","type":"group","bounds":{"x":0,"y":0,"width":1080,"height":1080},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"childIds":["background"]},
                {"id":"background","name":"背景","type":"rect","bounds":{"x":0,"y":0,"width":1080,"height":1080},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"fill":{"kind":"solid","color":"#FFFFFF"},"cornerRadius":0}],
            "fonts":[],"assets":[],"slots":[],"variants":[{"id":"square","label":"正方形 1080 × 1080","width":1080,"height":1080,"approvalStatus":"draft","nodePatches":[]}]
        })).expect("static document JSON").as_bytes())?;
        let bundled = std::fs::read(self.fixtures.join("documents/basic.json"))
            .ok()
            .and_then(|bytes| load_document(&bytes).ok());
        if let Some(bundled) = bundled {
            doc.fonts = bundled.fonts;
        }
        let workspace = self.create_workspace()?;
        if !doc.fonts.is_empty() {
            let source = ResourceStore::open(&self.fixtures.join("resources")).map_err(|i| vec![i])?;
            copy_resources(&doc, &source, &workspace)?;
        }
        self.insert(
            doc,
            ResourceStore::open(&workspace).map_err(|i| vec![i])?,
            workspace,
            None,
            Vec::new(),
        )
    }
}

pub struct WorkspaceLease {
    shared: SharedHost,
    id: String,
}
impl Drop for WorkspaceLease {
    fn drop(&mut self) {
        if let Ok(mut host) = self.shared.lock() {
            host.in_flight.remove(&self.id);
        }
    }
}

pub fn copy_resources(doc: &DesignDocument, source: &ResourceStore, destination: &Path) -> HostResult<()> {
    for asset in &doc.assets {
        if let Some(sha) = &asset.sha256 {
            let ext = asset.mime_hint.extension();
            let (bytes, _) = source
                .read("assets", sha, &[ext], design_render::resources::MAX_IMAGE_BYTES)
                .map_err(|e| vec![e.into_issue("bundled asset", Code::AssetMissing)])?;
            std::fs::write(destination.join("assets").join(format!("{sha}.{ext}")), &*bytes).map_err(|_| {
                error(
                    Code::PathDenied,
                    "cannot copy bundled resource",
                    "Check local workspace permissions.",
                )
            })?;
        }
    }
    for font in &doc.fonts {
        if let Some(sha) = &font.sha256 {
            let (bytes, ext) = source
                .read("fonts", sha, &["otf", "ttf", "ttc"], design_render::resources::MAX_FONT_BYTES)
                .map_err(|e| vec![e.into_issue("bundled font", Code::FontMissing)])?;
            std::fs::write(destination.join("fonts").join(format!("{sha}.{ext}")), &*bytes)
                .map_err(|_| error(Code::PathDenied, "cannot copy bundled font", "Check local workspace permissions."))?;
        }
    }
    Ok(())
}

// This short-lived IPC transaction owns the already decoded command; boxing
// it would add an allocation without changing retained session memory.
#[allow(clippy::large_enum_variant)]
pub enum Mutation {
    Edit(Command),
    Undo,
    Redo,
    Mode(Mode),
}

pub fn mutate(
    host: &mut Host,
    session_id: &str,
    expected_revision: u64,
    transaction_id: &str,
    mutation: Mutation,
) -> HostResult<SessionView> {
    let description = match &mutation {
        Mutation::Edit(command) => serde_json::to_vec(&("edit", expected_revision, command)),
        Mutation::Undo => serde_json::to_vec(&("undo", expected_revision)),
        Mutation::Redo => serde_json::to_vec(&("redo", expected_revision)),
        Mutation::Mode(mode) => serde_json::to_vec(&("mode", expected_revision, mode)),
    }
    .map_err(|_| error(Code::Schema, "cannot encode edit command", "Check the typed command."))?;
    if description.len() > 20 * 1024 * 1024 {
        return Err(error(
            Code::ResourceLimit,
            "edit transaction is too large",
            "Split the operation into smaller edits.",
        ));
    }
    let digest = sha256_hex(&description);
    let record = host.record_mut(session_id)?;
    if let Some(replay) = record.replay(transaction_id, &digest)? {
        return Ok(replay);
    }
    match mutation {
        Mutation::Edit(command) => record.session.apply(expected_revision, &command)?,
        Mutation::Undo => record.session.undo(expected_revision)?,
        Mutation::Redo => record.session.redo(expected_revision)?,
        Mutation::Mode(mode) => record.session.set_mode(expected_revision, mode)?,
    };
    record.issues.clear();
    let view = record.view(session_id);
    record.remember(transaction_id, digest, view.clone());
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retries_are_idempotent_and_stale_or_reused_ids_fail() {
        let workspace = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(opaque_id("host-test"));
        let mut host = Host::new(workspace.clone(), PathBuf::new()).unwrap();
        let view = host.new_document().unwrap();
        let id = &view.session_id;
        let command = Command::SetOpacity {
            node_id: "background".into(),
            opacity: 0.5,
        };
        let first = mutate(&mut host, id, 1, "transaction-1", Mutation::Edit(command.clone())).unwrap();
        let retry = mutate(&mut host, id, 1, "transaction-1", Mutation::Edit(command)).unwrap();
        assert_eq!(first.document, retry.document);
        assert_eq!(host.record(id).unwrap().session.revision(), 2);
        assert!(mutate(&mut host, id, 1, "transaction-2", Mutation::Undo).is_err());
        assert!(mutate(&mut host, id, 2, "transaction-1", Mutation::Undo).is_err());
        let undo = mutate(&mut host, id, 2, "transaction-3", Mutation::Undo).unwrap();
        assert_eq!(undo.document.revision, 3);
        let redo = mutate(&mut host, id, 3, "transaction-4", Mutation::Redo).unwrap();
        assert_eq!(redo.document.revision, 4);
        std::fs::remove_dir_all(workspace).unwrap();
    }
    #[test]
    fn another_process_cannot_share_live_workspace_cleanup_state() {
        let workspace = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(opaque_id("workspace-lock-test"));
        let first = Host::new(workspace.clone(), PathBuf::new()).unwrap();
        assert!(Host::new(workspace.clone(), PathBuf::new()).is_err());
        drop(first);
        let reopened = Host::new(workspace.clone(), PathBuf::new()).unwrap();
        drop(reopened);
        std::fs::remove_dir_all(workspace).unwrap();
    }
}
