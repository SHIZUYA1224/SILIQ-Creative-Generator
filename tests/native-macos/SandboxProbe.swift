import Foundation

// Executed as a separately signed app, never against a user's project. This
// catches file-access regressions that an unsandboxed Rust test cannot expose.
private let probeID = "jp.siliq.creative-generator.sandbox-tests"

private struct ProbeFailure: Error, CustomStringConvertible {
    let description: String
}

private func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw ProbeFailure(description: message) }
}

private func object(_ value: Any?) throws -> [String: Any] {
    guard let value = value as? [String: Any] else {
        throw ProbeFailure(description: "Expected a JSON object")
    }
    return value
}

private func string(_ value: Any?) throws -> String {
    guard let value = value as? String else {
        throw ProbeFailure(description: "Expected a JSON string")
    }
    return value
}

private func canonical(_ value: Any) throws -> Data {
    try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
}

private final class Broker {
    private let handle: OpaquePointer
    init(workspace: URL, fixtures: URL) throws {
        guard let handle = workspace.path.withCString({ workspace in
            fixtures.path.withCString { fixtures in siliq_native_create(workspace, fixtures) }
        }) else {
            let buffer = siliq_native_last_error()
            let reply = try Self.decode(buffer)
            throw ProbeFailure(description: "create: \(String(decoding: reply, as: UTF8.self))")
        }
        self.handle = handle
    }
    deinit { siliq_native_close(handle) }
    private static func decode(_ buffer: SiliqNativeBuffer) throws -> Data {
        defer { siliq_native_free(buffer) }
        guard let pointer = buffer.data, buffer.len > 0, buffer.len <= 64 * 1024 * 1024 else {
            throw ProbeFailure(description: "Invalid native response buffer")
        }
        return Data(bytes: pointer, count: buffer.len)
    }
    func call(_ command: String, _ args: [String: Any] = [:]) throws -> [String: Any] {
        let request = try canonical(["command": command, "args": args])
        let response = try request.withUnsafeBytes { bytes in
            try Self.decode(siliq_native_request(handle, bytes.bindMemory(to: UInt8.self).baseAddress, request.count))
        }
        let envelope = try object(JSONSerialization.jsonObject(with: response))
        try require(envelope["ok"] as? Bool == true, "\(command): \(String(decoding: response, as: UTF8.self))")
        return try object(envelope["value"])
    }
}

@main
private enum SandboxProbe {
    static func main() {
        let began = Date()
        do {
            let result = try exercise()
            var evidence = result
            evidence["ok"] = true
            evidence["durationSeconds"] = Date().timeIntervalSince(began)
            print(String(decoding: try canonical(evidence), as: UTF8.self))
        } catch {
            let evidence: [String: Any] = ["ok": false, "appId": probeID, "error": String(describing: error)]
            if let data = try? canonical(evidence) { print(String(decoding: data, as: UTF8.self)) }
            exit(1)
        }
    }

    private static func exercise() throws -> [String: Any] {
        let files = FileManager.default
        try require(Bundle.main.bundleIdentifier == probeID, "Probe bundle identifier differs")
        let home = URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
        let expected = "/Library/Containers/\(probeID)/Data"
        try require(home.path.hasSuffix(expected), "Foundation did not select the probe's App Sandbox container")
        guard let support = files.urls(for: .applicationSupportDirectory, in: .userDomainMask).first,
              let resources = Bundle.main.resourceURL else {
            throw ProbeFailure(description: "Application Support or bundled resources are unavailable")
        }
        try require(support.path.hasPrefix(home.path + "/"), "Application Support is outside the probe container")
        let run = support.appendingPathComponent("siliq-sandbox-probe/\(UUID().uuidString)", isDirectory: true)
        let workspace = run.appendingPathComponent("workspace", isDirectory: true)
        let output = run.appendingPathComponent("output", isDirectory: true)
        let batchOutput = output.appendingPathComponent("batch", isDirectory: true)
        try files.createDirectory(at: batchOutput, withIntermediateDirectories: true)
        let fixtures = resources.appendingPathComponent("public-safe", isDirectory: true)
        let broker = try Broker(workspace: workspace, fixtures: fixtures)
        var steps = [String]()

        let initial = try broker.call("open_fixture", ["fixture": "basic"])
        let session = try string(initial["sessionId"])
        let edited = try broker.call("apply_edit", ["sessionId": session, "expectedRevision": 1,
            "transactionId": "sandbox-edit", "command": ["op": "setText", "nodeId": "headline", "value": "Sandbox probe"]])
        try require(try object(edited["document"])["revision"] as? Int == 2, "Edit revision differs")
        let approved = try broker.call("apply_edit", ["sessionId": session, "expectedRevision": 2,
            "transactionId": "sandbox-approval", "command": ["op": "setVariantApproval", "variantId": "square", "approvalStatus": "approved"]])
        let document = try object(approved["document"])
        try require(document["revision"] as? Int == 3, "Approval revision differs")
        steps.append("bundled-fixture-edit-and-recovery-checkpoint")

        let project = output.appendingPathComponent("probe.siliqcg")
        _ = try broker.call("save_project_path", ["sessionId": session, "expectedRevision": 3, "path": project.path, "overwrite": false])
        let reopened = try broker.call("open_project_path", ["path": project.path])
        try require(try canonical(object(reopened["document"])) == canonical(document), "Saved and opened document differ")
        let reopenedSession = try string(reopened["sessionId"])
        steps.append("project-save-and-open")

        let preview = try broker.call("native_preview", ["sessionId": reopenedSession, "revision": 3,
            "variantId": "square", "requestId": 1])
        try require(try string(preview["pngBase64"]).hasPrefix("iVBORw0KGgo"), "Native preview is not a PNG")
        var recipes = [[String: Any]]()
        for format in ["png", "jpeg", "webp", "svg", "pdf"] {
            let recipe: [String: Any] = ["format": format, "quality": format == "jpeg" ? 90 : NSNull(),
                "jpegBackground": format == "jpeg" ? "#FFFFFF" : NSNull(), "webpLossless": true,
                "allowRasterization": format == "svg" || format == "pdf", "pdfDpi": 144]
            recipes.append(recipe)
            if format == "png" || format == "pdf" {
                let path = output.appendingPathComponent("probe.\(format)")
                _ = try broker.call("export_design_path", ["sessionId": reopenedSession, "revision": 3,
                    "variantId": "square", "recipe": recipe, "path": path.path])
                let contents = try Data(contentsOf: path)
                let signature = format == "png" ? Data([137, 80, 78, 71, 13, 10, 26, 10]) : Data("%PDF-".utf8)
                try require(contents.count > 100 && contents.starts(with: signature), "Export signature differs: \(format)")
            }
        }
        steps.append("canonical-preview-and-png-pdf-export")

        let rows = output.appendingPathComponent("rows.json")
        try canonical([["id": "001", "text": "Sandbox batch", "image": "photo"]]).write(to: rows, options: [.withoutOverwriting])
        let dataset = try broker.call("select_dataset_path", ["sessionId": reopenedSession, "path": rows.path])
        try require(dataset["rowCount"] as? Int == 1, "Dataset row count differs")
        let plan = try broker.call("preflight_batch_path", ["sessionId": reopenedSession, "revision": 3,
            "datasetId": try string(dataset["datasetId"]), "mapping": ["id": "rowId", "text": "headline-slot", "image": "photo-slot"],
            "variantIds": ["square"], "namePattern": "{rowId}-{variantId}-{format}.{format}",
            "errorPolicy": "block-all", "recipes": recipes, "outputDirectory": batchOutput.path])
        try require(plan["total"] as? Int == 5, "Plan did not include five derivative formats")
        let thumbnail = try broker.call("native_batch_preview", ["planId": try string(plan["previewPlanId"]), "itemIndex": 0])
        try require(try string(thumbnail["pngBase64"]).hasPrefix("iVBORw0KGgo"), "Batch preview is not a PNG")
        let job = try broker.call("start_batch", ["planId": try string(plan["planId"])])
        let jobID = try string(job["jobId"])
        let deadline = Date().addingTimeInterval(60)
        var terminal: [String: Any] = [:]
        while Date() < deadline {
            terminal = try broker.call("batch_status", ["jobId": jobID])
            if terminal["status"] as? String == "completed" { break }
            let status = try string(terminal["status"])
            try require(!["failed", "cancelled", "completed_with_errors"].contains(status), "Batch did not complete: \(terminal)")
            Thread.sleep(forTimeInterval: 0.02)
        }
        try require(terminal["status"] as? String == "completed" && terminal["completed"] as? Int == 5, "Batch did not finish five files before its deadline")
        let manifest = try object(JSONSerialization.jsonObject(with: Data(contentsOf: batchOutput.appendingPathComponent("manifest.json"))))
        guard let items = manifest["items"] as? [[String: Any]] else { throw ProbeFailure(description: "Manifest items are missing") }
        try require(manifest["status"] as? String == "completed" && items.count == 5, "Terminal manifest differs")
        var formats = Set<String>()
        for item in items {
            try require(item["rowId"] as? String == "001", "String row identifier lost its leading zero")
            let filename = try string(item["file"])
            try require(!filename.contains("/") && !filename.contains("\\") && filename != "." && filename != "..", "Unexpected manifest output name")
            let contents = try Data(contentsOf: batchOutput.appendingPathComponent(filename))
            try require(contents.count > 100, "Manifest output is empty")
            let format = try string(item["format"])
            let validSignature: Bool
            switch format {
            case "png": validSignature = contents.starts(with: [137, 80, 78, 71, 13, 10, 26, 10])
            case "jpeg": validSignature = contents.starts(with: [255, 216, 255])
            case "webp": validSignature = contents.starts(with: Data("RIFF".utf8)) && contents.dropFirst(8).starts(with: Data("WEBP".utf8))
            case "pdf": validSignature = contents.starts(with: Data("%PDF-".utf8))
            case "svg": validSignature = String(data: contents, encoding: .utf8)?.contains("<svg") == true
            default: validSignature = false
            }
            try require(validSignature, "Batch output signature differs: \(format)")
            formats.insert(format)
        }
        try require(formats == Set(["png", "jpeg", "webp", "svg", "pdf"]), "Manifest does not contain every requested format")
        _ = try broker.call("storage_status")
        steps.append("json-five-format-batch-terminal-manifest")
        // Retain this uniquely named run as private local evidence. Nothing in
        // the product container or the user's project folders is inspected.
        return ["appId": probeID, "homeIsContainer": true, "applicationSupportIsContainer": true,
                "steps": steps, "manifestStatus": "completed", "batchOutputs": items.count,
                "formats": formats.sorted(), "outputSignaturesVerified": true]
    }
}
