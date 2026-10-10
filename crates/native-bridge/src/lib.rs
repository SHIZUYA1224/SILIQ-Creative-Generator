//! C ABI for a local native UI. Handles are registry tokens rather than
//! dereferenced foreign pointers; pending calls retain their broker across close.
use design_model::{Code, Issue};
use siliq_desktop::native::{MAX_REQUEST_BYTES, NativeBroker, error_response};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{c_char, c_void},
    path::PathBuf,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

static HANDLES: LazyLock<Mutex<BTreeMap<usize, Arc<NativeBroker>>>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));
static NEXT_HANDLE: AtomicUsize = AtomicUsize::new(1);
thread_local! { static LAST_ERROR: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) }; }

#[repr(C)]
pub struct SiliqNativeBuffer {
    pub data: *mut u8,
    pub len: usize,
}
fn buffer(bytes: Vec<u8>) -> SiliqNativeBuffer {
    let mut bytes = bytes.into_boxed_slice();
    let result = SiliqNativeBuffer {
        data: bytes.as_mut_ptr(),
        len: bytes.len(),
    };
    std::mem::forget(bytes);
    result
}
fn failed(code: Code, message: &str) -> Vec<u8> {
    error_response(vec![Issue::error(
        code,
        message,
        "Retry the native operation or inspect its structured issues.",
    )])
}

unsafe fn path_text(pointer: *const c_char) -> Result<PathBuf, Vec<u8>> {
    if pointer.is_null() {
        return Err(failed(Code::PathDenied, "native path is null"));
    }
    let mut length = 0;
    while length < 4096 {
        // SAFETY: the C ABI requires a readable NUL-terminated C string.
        if unsafe { *pointer.add(length) } == 0 {
            break;
        }
        length += 1;
    }
    if length == 4096 {
        return Err(failed(Code::ResourceLimit, "native path exceeds 4095 UTF-8 bytes"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer.cast::<u8>(), length) };
    let text = std::str::from_utf8(bytes).map_err(|_| failed(Code::PathDenied, "native path is not UTF-8"))?;
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err(failed(Code::PathDenied, "native path must be absolute"));
    }
    Ok(path)
}

/// Create a broker without initializing a WebView or a server.
/// # Safety
/// Each pointer must address a readable NUL-terminated UTF-8 path string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siliq_native_create(workspace: *const c_char, fixtures: *const c_char) -> *mut c_void {
    let result = std::panic::catch_unwind(|| {
        let workspace = unsafe { path_text(workspace) }?;
        let fixtures = unsafe { path_text(fixtures) }?;
        let broker = NativeBroker::new(workspace, fixtures).map_err(error_response)?;
        let mut handles = HANDLES
            .lock()
            .map_err(|_| failed(Code::RenderFailed, "native handle registry is unavailable"))?;
        if handles.len() >= 4 {
            return Err(failed(Code::ResourceLimit, "four native brokers are already open"));
        }
        let id = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        if id == 0 {
            return Err(failed(Code::ResourceLimit, "native handle range exhausted"));
        }
        handles.insert(id, Arc::new(broker));
        Ok(id)
    });
    match result {
        Ok(Ok(id)) => {
            LAST_ERROR.with(|e| e.borrow_mut().clear());
            std::ptr::with_exposed_provenance_mut(id)
        }
        Ok(Err(bytes)) => {
            LAST_ERROR.with(|e| *e.borrow_mut() = bytes);
            std::ptr::null_mut()
        }
        Err(_) => {
            LAST_ERROR.with(|e| *e.borrow_mut() = failed(Code::RenderFailed, "native creation was interrupted"));
            std::ptr::null_mut()
        }
    }
}

/// Execute a typed command and return an owned JSON envelope.
/// # Safety
/// For nonzero length, request must address that many readable bytes. The caller
/// must release the returned buffer exactly once with siliq_native_free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siliq_native_request(handle: *mut c_void, request: *const u8, length: usize) -> SiliqNativeBuffer {
    let result = std::panic::catch_unwind(|| {
        if request.is_null() || length == 0 || length > MAX_REQUEST_BYTES {
            return failed(Code::ResourceLimit, "native request must contain 1 byte to 20 MiB");
        }
        let broker = HANDLES.lock().ok().and_then(|h| h.get(&handle.addr()).cloned());
        let Some(broker) = broker else {
            return failed(Code::PathDenied, "native broker handle is unknown or closed");
        };
        broker.request(unsafe { std::slice::from_raw_parts(request, length) })
    });
    buffer(result.unwrap_or_else(|_| failed(Code::RenderFailed, "native request was interrupted; reload the canonical session")))
}

/// Return a copy of the last creation error on this thread.
#[unsafe(no_mangle)]
pub extern "C" fn siliq_native_last_error() -> SiliqNativeBuffer {
    buffer(LAST_ERROR.with(|e| {
        if e.borrow().is_empty() {
            br#"{"ok":true,"value":null}"#.to_vec()
        } else {
            e.borrow().clone()
        }
    }))
}

/// Release one response buffer.
/// # Safety
/// The buffer must be an unmodified, unreleased result from this library.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn siliq_native_free(response: SiliqNativeBuffer) {
    if !response.data.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(response.data, response.len)) });
    }
}

/// Close a handle. Pending requests keep their existing broker alive.
#[unsafe(no_mangle)]
pub extern "C" fn siliq_native_close(handle: *mut c_void) {
    if let Ok(mut handles) = HANDLES.lock() {
        handles.remove(&handle.addr());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(handle: *mut c_void, command: &str, args: serde_json::Value) -> serde_json::Value {
        let request = serde_json::to_vec(&serde_json::json!({"command":command,"args":args})).unwrap();
        let reply = unsafe { siliq_native_request(handle, request.as_ptr(), request.len()) };
        let result: serde_json::Value = serde_json::from_slice(unsafe { std::slice::from_raw_parts(reply.data, reply.len) }).unwrap();
        unsafe { siliq_native_free(reply) };
        assert_eq!(result["ok"], true, "{command}: {result}");
        result["value"].clone()
    }
    #[test]
    fn ffi_load_edit_preview_save_open_five_exports_and_batch_complete() {
        let root = std::fs::canonicalize(std::env::temp_dir()).unwrap().join(format!(
            "siliq-native-ffi-{}-{}",
            std::process::id(),
            NEXT_HANDLE.load(Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/public-safe");
        let workspace = std::ffi::CString::new(root.join("workspace").to_str().unwrap()).unwrap();
        let fixtures = std::ffi::CString::new(fixture.to_str().unwrap()).unwrap();
        let handle = unsafe { siliq_native_create(workspace.as_ptr(), fixtures.as_ptr()) };
        assert!(!handle.is_null());
        for request in [
            br#"{"command":"new_document","command":"new_document","args":{}}"#.as_slice(),
            br#"{"command":"new_document","args":{"unused":1}}"#,
            br#"{"command":"unknown","args":{}}"#,
            br#"{"command":"read_session","args":{"sessionId":"a","sessionId":"b"}}"#,
            br#"{"command":"new_document","args":{},"extra":true}"#,
        ] {
            let reply = unsafe { siliq_native_request(handle, request.as_ptr(), request.len()) };
            let rejected: serde_json::Value = serde_json::from_slice(unsafe { std::slice::from_raw_parts(reply.data, reply.len) }).unwrap();
            assert_eq!(rejected["ok"], false);
            unsafe { siliq_native_free(reply) };
        }
        let view = call(handle, "open_fixture", serde_json::json!({"fixture":"basic"}));
        let session = view["sessionId"].as_str().unwrap();
        let changed = call(
            handle,
            "apply_edit",
            serde_json::json!({"sessionId":session,"expectedRevision":1,"transactionId":"ffi-edit-1","command":{"op":"setText","nodeId":"headline","value":"Bridge test"}}),
        );
        assert_eq!(changed["document"]["revision"], 2);
        let retry = call(
            handle,
            "apply_edit",
            serde_json::json!({"sessionId":session,"expectedRevision":1,"transactionId":"ffi-edit-1","command":{"op":"setText","nodeId":"headline","value":"Bridge test"}}),
        );
        assert_eq!(retry, changed);
        let preview = call(
            handle,
            "native_preview",
            serde_json::json!({"sessionId":session,"revision":2,"variantId":"square","requestId":1}),
        );
        assert!(preview["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"));
        assert_eq!(preview["metadata"]["width"], 1080);
        let approved = call(
            handle,
            "apply_edit",
            serde_json::json!({"sessionId":session,"expectedRevision":2,"transactionId":"ffi-approve-1","command":{"op":"setVariantApproval","variantId":"square","approvalStatus":"approved"}}),
        );
        assert_eq!(approved["document"]["revision"], 3);
        let project = root.join("project.siliqcg");
        call(
            handle,
            "save_project_path",
            serde_json::json!({"sessionId":session,"expectedRevision":3,"path":project,"overwrite":false}),
        );
        let reopened = call(handle, "open_project_path", serde_json::json!({"path":project}));
        assert_eq!(reopened["document"], approved["document"]);
        let session = reopened["sessionId"].as_str().unwrap();
        let mut recipes = Vec::new();
        for (format, extension) in [("png", "png"), ("jpeg", "jpg"), ("webp", "webp"), ("svg", "svg"), ("pdf", "pdf")] {
            let recipe = serde_json::json!({"format":format,"quality":if format=="jpeg"{Some(90)}else{None},"jpegBackground":if format=="jpeg"{Some("#FFFFFF")}else{None},"webpLossless":true,"allowRasterization":format=="svg"||format=="pdf","pdfDpi":144});
            let path = root.join(format!("image.{extension}"));
            let report = call(
                handle,
                "export_design_path",
                serde_json::json!({"sessionId":session,"revision":3,"variantId":"square","recipe":recipe,"path":path}),
            );
            assert!(path.metadata().unwrap().len() > 100);
            assert!(report["export"]["outputSha256"].as_str().is_some());
            recipes.push(recipe);
        }
        let json_path = root.join("rows.json");
        std::fs::write(&json_path, br#"[{"id":"001","text":"Quality","image":"photo"}]"#).unwrap();
        let dataset = call(
            handle,
            "select_dataset_path",
            serde_json::json!({"sessionId":session,"path":json_path}),
        );
        assert_eq!(dataset["rowCount"], 1);
        let output = root.join("batch");
        std::fs::create_dir(&output).unwrap();
        let plan = call(
            handle,
            "preflight_batch_path",
            serde_json::json!({"sessionId":session,"revision":3,"datasetId":dataset["datasetId"],"mapping":{"id":"rowId","text":"headline-slot","image":"photo-slot"},"variantIds":["square"],"namePattern":"{rowId}-{variantId}-{format}.{format}","errorPolicy":"block-all","recipes":recipes,"outputDirectory":output}),
        );
        assert_eq!(plan["total"], 5);
        assert!(plan["planId"].as_str().is_some(), "{plan}");
        let thumbnail = call(
            handle,
            "native_batch_preview",
            serde_json::json!({"planId":plan["previewPlanId"],"itemIndex":0}),
        );
        assert!(thumbnail["pngBase64"].as_str().unwrap().starts_with("iVBORw0KGgo"));
        let job = call(handle, "start_batch", serde_json::json!({"planId":plan["planId"]}));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let status = call(handle, "batch_status", serde_json::json!({"jobId":job["jobId"]}));
            if status["status"] == "completed" {
                assert_eq!(status["completed"], 5);
                break;
            }
            assert!(status["status"] != "failed", "{status}");
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(output.join("manifest.json").is_file());
        call(handle, "storage_status", serde_json::json!({}));
        siliq_native_close(handle);
        let request = br#"{"command":"read_session","args":{"sessionId":"closed"}}"#;
        let reply = unsafe { siliq_native_request(handle, request.as_ptr(), request.len()) };
        let result: serde_json::Value = serde_json::from_slice(unsafe { std::slice::from_raw_parts(reply.data, reply.len) }).unwrap();
        assert_eq!(result["ok"], false);
        unsafe { siliq_native_free(reply) };
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reimport_returns_the_registered_id_and_preserves_catalog_order_and_retry() {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("siliq-native-reimport-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let fixture = std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/public-safe")).unwrap();
        let workspace = std::ffi::CString::new(root.join("workspace").to_str().unwrap()).unwrap();
        let fixtures = std::ffi::CString::new(fixture.to_str().unwrap()).unwrap();
        let handle = unsafe { siliq_native_create(workspace.as_ptr(), fixtures.as_ptr()) };
        assert!(!handle.is_null());
        let view = call(handle, "open_fixture", serde_json::json!({"fixture":"basic"}));
        let session = view["sessionId"].as_str().unwrap();
        let raster = view["document"]["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["kind"] == "raster")
            .unwrap();
        let source = root.join("image.png");
        std::fs::copy(
            fixture.join(format!("resources/assets/{}.png", raster["sha256"].as_str().unwrap())),
            &source,
        )
        .unwrap();
        let first = call(
            handle,
            "import_asset_path",
            serde_json::json!({"sessionId":session,"expectedRevision":1,"transactionId":"import-first","kind":"raster","path":source}),
        );
        let first_id = first["importedResourceId"].as_str().unwrap().to_owned();
        let svg = root.join("other.svg");
        std::fs::write(&svg,br##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><path d="M0 0L32 0L32 32Z" fill="#FF0000"/></svg>"##).unwrap();
        let second = call(
            handle,
            "import_asset_path",
            serde_json::json!({"sessionId":session,"expectedRevision":2,"transactionId":"import-second","kind":"svg","path":svg}),
        );
        assert_ne!(second["importedResourceId"], first_id);
        let before = second["document"]["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].clone())
            .collect::<Vec<_>>();
        let args =
            serde_json::json!({"sessionId":session,"expectedRevision":3,"transactionId":"import-again","kind":"raster","path":source});
        let again = call(handle, "import_asset_path", args.clone());
        assert_eq!(again["importedResourceId"], first_id);
        let after = again["document"]["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].clone())
            .collect::<Vec<_>>();
        assert_eq!(before, after);
        assert_ne!(after.last().unwrap(), &again["importedResourceId"]);
        std::fs::remove_file(source).unwrap();
        assert_eq!(call(handle, "import_asset_path", args), again);
        siliq_native_close(handle);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_buffers_and_closed_handles_return_owned_errors() {
        let reply = unsafe { siliq_native_request(std::ptr::null_mut(), std::ptr::null(), 0) };
        let parsed: serde_json::Value = serde_json::from_slice(unsafe { std::slice::from_raw_parts(reply.data, reply.len) }).unwrap();
        assert_eq!(parsed["ok"], false);
        unsafe { siliq_native_free(reply) };
        let reply = unsafe { siliq_native_request(std::ptr::null_mut(), b"{}".as_ptr(), 2) };
        assert!(reply.len > 0);
        unsafe { siliq_native_free(reply) };
    }
}
