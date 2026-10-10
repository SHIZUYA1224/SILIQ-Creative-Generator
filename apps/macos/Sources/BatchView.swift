import AppKit
import Foundation
import SwiftUI

private struct NativeDataset {
    let id: String
    let columns: [String]
    let rows: Int
    let samples: [[String: Any]]
    init(_ raw: [String: Any]) {
        id = jsonString(raw["datasetId"])
        columns = raw["columns"] as? [String] ?? []
        rows = Int(jsonNumber(raw["rowCount"]))
        samples = jsonObjects(raw["sampleRows"])
    }
}
private struct NativeCandidate: Identifiable {
    let id: Int
    let rowID: String
    let variantID: String
    let file: String
    let format: String
    let issues: [StudioIssue]
    init(_ raw: [String: Any], index: Int) {
        id = index; rowID = jsonString(raw["rowId"]); variantID = jsonString(raw["variantId"])
        file = jsonString(raw["file"]); format = jsonString(raw["format"])
        issues = jsonObjects(raw["issues"]).map(StudioIssue.init)
    }
}
private struct NativeBatchPlan {
    let id: String
    let previewID: String
    let rows: Int
    let total: Int
    let variants: [String]
    let candidates: [NativeCandidate]
    let issues: [StudioIssue]
    let validOnly: Bool
    init(_ raw: [String: Any]) {
        id = jsonString(raw["planId"]); previewID = jsonString(raw["previewPlanId"], fallback: id)
        rows = Int(jsonNumber(raw["rows"])); total = Int(jsonNumber(raw["total"]))
        variants = raw["variantIds"] as? [String] ?? []
        candidates = jsonObjects(raw["items"]).enumerated().map { NativeCandidate($0.element, index: $0.offset) }
        issues = jsonObjects(raw["issues"]).map(StudioIssue.init)
        validOnly = jsonString(raw["errorPolicy"]) == "valid-only"
    }
}
private struct NativeBatchJob {
    let id: String
    let status: String
    let total: Int
    let completed: Int
    let rowID: String
    let variantID: String
    let issues: [StudioIssue]
    var active: Bool { ["queued", "ready", "preflighting", "running", "cancelling"].contains(status) }
    var resumable: Bool { ["cancelled", "interrupted", "failed"].contains(status) }
    init(_ raw: [String: Any]) {
        id = jsonString(raw["jobId"]); status = jsonString(raw["status"])
        total = Int(jsonNumber(raw["total"])); completed = Int(jsonNumber(raw["completed"]))
        rowID = jsonString(raw["currentRowId"]); variantID = jsonString(raw["currentVariantId"])
        issues = jsonObjects(raw["issues"]).map(StudioIssue.init)
    }
}
private struct NativeSavedBatch: Identifiable {
    let id: String
    let title: String
    let revision: Int
    let total: Int
    init(_ raw: [String: Any]) {
        id = jsonString(raw["snapshotId"]); title = jsonString(raw["documentTitle"])
        revision = Int(jsonNumber(raw["revision"])); total = Int(jsonNumber(raw["total"]))
    }
}

// A sheet can close while the native worker continues. Keep its controls and
// OS grant alive across sheet recreation, rather than losing its job handle.
@MainActor private final class NativeBatchState: ObservableObject {
    static let shared = NativeBatchState()
    @Published var sessionID = ""
    @Published var dataset: NativeDataset?
    @Published var mapping: [String: String] = [:]
    @Published var variants: Set<String> = []
    @Published var formats: Set<String> = ["png"]
    @Published var pattern = "{rowId}_{variantId}.{format}"
    @Published var validOnly = false
    @Published var rasterization = false
    @Published var jpegBackground = "#FFFFFF"
    @Published var plan: NativeBatchPlan?
    @Published var job: NativeBatchJob?
    @Published var jobTitle = ""
    @Published var saved: [NativeSavedBatch] = []
    @Published var savedID = ""
    @Published var issues: [StudioIssue] = []
    @Published var working = false
    private var outputURL: URL?
    private var scoped = false
    private var jobGrants: [String: (url: URL, scoped: Bool)] = [:]
    var active: Bool { job?.active ?? false }
    var pollKey: String { active ? (job?.id ?? "") : "" }
    func reset(_ session: SessionView?) {
        guard sessionID != (session?.sessionId ?? "") else { return }
        sessionID = session?.sessionId ?? ""; dataset = nil; mapping = [:]; plan = nil; savedID = ""; issues = []
        variants = Set(session?.document.variants.filter { jsonString($0["approvalStatus"]) == "approved" }.map { jsonString($0["id"]) } ?? [])
        if job == nil || job?.status == "completed" || job?.status == "completed_with_errors" { releaseGrant() }
    }
    func retainGrant(_ url: URL) {
        releaseGrant(); outputURL = url; scoped = url.startAccessingSecurityScopedResource()
    }
    func releaseGrant() {
        if scoped { outputURL?.stopAccessingSecurityScopedResource() }
        outputURL = nil; scoped = false
    }
    func setJob(_ raw: [String: Any]) {
        let next = NativeBatchJob(raw)
        if !next.id.isEmpty, jobGrants[next.id] == nil, let url = outputURL {
            jobGrants[next.id] = (url: url, scoped: scoped)
            outputURL = nil; scoped = false
        }
        job = next
        if next.status == "completed" || next.status == "completed_with_errors" {
            if let grant = jobGrants.removeValue(forKey: next.id), grant.scoped { grant.url.stopAccessingSecurityScopedResource() }
        }
    }
}

@MainActor struct BatchView: View {
    @ObservedObject var model: StudioModel
    @ObservedObject private var state = NativeBatchState.shared
    @Environment(\.dismiss) private var dismiss
    private let allFormats = ["png", "jpeg", "webp", "pdf", "svg"]
    private var disabled: Bool { state.working || model.busy || state.active }
    private var session: SessionView? { model.session }
    private var needsRasterization: Bool { state.formats.contains("pdf") || state.formats.contains("svg") }
    private var canInspect: Bool {
        !disabled && state.dataset != nil && !state.variants.isEmpty && !state.formats.isEmpty && (!needsRasterization || state.rasterization)
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 5) {
                    Text("データから量産").font(.title2.weight(.semibold))
                    Text("承認したデザインと入力を固定し、まとめて書き出します。").foregroundStyle(.secondary)
                }
                Spacer()
                if state.working { ProgressView().controlSize(.small) }
                Button("閉じる") { dismiss() }.keyboardShortcut(.cancelAction).disabled(state.working)
            }
            HSplitView {
                ScrollView { controls.padding(.trailing, 14) }.frame(minWidth: 350, idealWidth: 390, maxWidth: 440)
                ScrollView { results.padding(.leading, 14) }.frame(minWidth: 520, maxWidth: .infinity)
            }
            savedJobs
            HStack {
                Text(state.active ? "量産は制作画面に戻っても続きます。" : "元のプロジェクトと既存出力は保護します。")
                    .font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(state.active ? "量産を続けて制作に戻る" : "制作に戻る") { dismiss() }.disabled(state.working)
            }
        }
        .padding(22).frame(minWidth: 1050, idealWidth: 1120, minHeight: 750, idealHeight: 820)
        .interactiveDismissDisabled(state.working)
        .task { state.reset(session); await refreshSaved() }
        .task(id: state.pollKey) { await pollJob() }
        .onChange(of: session?.sessionId) { _ in state.reset(session) }
        .onChange(of: session?.document.revision) { _ in state.plan = nil }
    }
    private var controls: some View {
        VStack(alignment: .leading, spacing: 18) {
            GroupBox {
                VStack(alignment: .leading, spacing: 10) {
                    Button { Task { await selectData() } } label: { Label("CSV / JSON を選ぶ", systemImage: "tablecells") }
                        .disabled(disabled || session == nil)
                    Text("UTF-8・先頭ゼロを保持。値を数式として実行しません。").font(.caption).foregroundStyle(.secondary)
                    if let dataset = state.dataset {
                        Label("\(dataset.rows) 行・\(dataset.columns.count) 列", systemImage: "checkmark.circle").foregroundStyle(.green)
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            } label: { Text("01 データを選ぶ") }
            if let dataset = state.dataset {
                GroupBox {
                    VStack(alignment: .leading, spacing: 9) {
                        ForEach(dataset.columns, id: \.self) { column in
                            Picker(column, selection: Binding(get: { state.mapping[column] ?? "" }, set: { state.mapping[column] = $0; state.plan = nil })) {
                                Text("使用しない").tag("")
                                Text("行 ID（ファイル名）").tag("rowId")
                                ForEach(Array((session?.document.slots ?? []).enumerated()), id: \.offset) { entry in
                                    let slot = entry.element, id = jsonString(slot["id"])
                                    Text(id + (jsonBool(slot["required"]) ? "（必須）" : "")).tag(id)
                                }
                            }.disabled(disabled)
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading)
                } label: { Text("02 列を差し替え欄に対応付ける") }
            }
            variantOptions
            formatOptions
            GroupBox {
                VStack(alignment: .leading, spacing: 9) {
                    TextField("出力名", text: invalidating(\NativeBatchState.pattern)).disabled(disabled)
                        .accessibilityLabel("量産の出力名")
                    Text("変数: {rowId} / {variantId} / {format}。形式に合わせて拡張子を付けます。").font(.caption).foregroundStyle(.secondary)
                    Toggle("エラーのある候補を除いて出力する", isOn: invalidating(\NativeBatchState.validOnly)).disabled(disabled)
                    Text("通常はエラーが1件でもあれば停止します。除外した候補も結果に記録します。").font(.caption).foregroundStyle(.secondary)
                }
            } label: { Text("05 出力名とエラーの扱い") }
            Button { Task { await inspect() } } label: { Label("全件検査・出力先を選択", systemImage: "checkmark.shield") }
                .buttonStyle(.borderedProminent).disabled(!canInspect)
            if state.working { Text("検査中はすべての候補を確認しています。大きなデータでは時間がかかります。").font(.caption).foregroundStyle(.secondary) }
        }
    }
    private var variantOptions: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(Array((session?.document.variants ?? []).enumerated()), id: \.offset) { entry in
                    let variant = entry.element, id = jsonString(variant["id"]), approved = jsonString(variant["approvalStatus"]) == "approved"
                    Toggle(jsonString(variant["label"], fallback: id) + (approved ? "" : "（未承認）"), isOn: membership(id, in: \NativeBatchState.variants))
                        .disabled(disabled || !approved)
                }
                if session?.document.variants.isEmpty ?? true { Text("作品を開いて承認済みサイズを設定してください。").font(.caption).foregroundStyle(.secondary) }
            }.frame(maxWidth: .infinity, alignment: .leading)
        } label: { Text("03 承認済みサイズ") }
    }
    private var formatOptions: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 9) {
                HStack { ForEach(allFormats, id: \.self) { format in Toggle(format.uppercased(), isOn: membership(format, in: \NativeBatchState.formats)).disabled(disabled) } }
                Text("WebPはロスレスです。").font(.caption).foregroundStyle(.secondary)
                if state.formats.contains("jpeg") {
                    TextField("JPEG背景色（#RRGGBB）", text: invalidating(\NativeBatchState.jpegBackground)).disabled(disabled)
                    Text("JPEGは品質90。透明部分をこの色で埋めます。").font(.caption).foregroundStyle(.secondary)
                }
                if needsRasterization {
                    Toggle("PDF / SVG 全体の画像化を許可する", isOn: invalidating(\NativeBatchState.rasterization)).disabled(disabled)
                    Text("文字・図形は編集できません。PDFはRGB校正用・144 DPIです。").font(.caption).foregroundStyle(.secondary)
                }
            }.frame(maxWidth: .infinity, alignment: .leading)
        } label: { Text("04 出力形式") }
    }
    private var results: some View {
        VStack(alignment: .leading, spacing: 18) {
            Text("プレビューと検査結果").font(.headline)
            if let dataset = state.dataset { dataPreview(dataset) }
            else { Label("データを取り込むと、先頭の入力行を確認できます。", systemImage: "tablecells").foregroundStyle(.secondary).padding(.vertical, 35) }
            if let plan = state.plan {
                GroupBox {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("\(plan.total) 件の候補").font(.title3.weight(.semibold))
                        Text("\(plan.rows) 行 × \(plan.variants.count) サイズ × \(state.formats.count) 形式").foregroundStyle(.secondary)
                        Text("この計画の入力は固定されています。開始後に元の作品を編集しても変わりません。").font(.caption).foregroundStyle(.secondary)
                        if !plan.previewID.isEmpty, !plan.candidates.isEmpty { NativeCandidateGallery(model: model, planID: plan.previewID, items: plan.candidates) }
                        Button {
                            Task { await operate { state.jobTitle = session?.document.title ?? ""; state.setJob(jsonObject(try await model.core.call("start_batch", ["planId": plan.id]))) } }
                        } label: { Label(plan.validOnly ? "正常な候補を書き出す" : "\(plan.total) 件を書き出す", systemImage: "play.fill") }
                            .buttonStyle(.borderedProminent)
                            .disabled(disabled || plan.id.isEmpty || (!plan.validOnly && plan.issues.contains { $0.severity == "error" }))
                    }.frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            if let job = state.job { progress(job) }
            NativeBatchIssueList(issues: state.issues)
        }.frame(maxWidth: .infinity, alignment: .leading)
    }
    private func dataPreview(_ dataset: NativeDataset) -> some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 9) {
                ScrollView(.horizontal) {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(alignment: .top) { ForEach(dataset.columns, id: \.self) { Text($0).fontWeight(.semibold).frame(width: 145, alignment: .leading) } }
                        Divider()
                        ForEach(Array(dataset.samples.enumerated()), id: \.offset) { entry in
                            HStack(alignment: .top) { ForEach(dataset.columns, id: \.self) { column in Text(jsonString(entry.element[column])).textSelection(.enabled).frame(width: 145, alignment: .leading) } }
                        }
                    }.font(.caption)
                }
                Text("先頭 \(dataset.samples.count) 行の入力内容").font(.caption).foregroundStyle(.secondary)
            }
        }
    }
    private func progress(_ job: NativeBatchJob) -> some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Text(statusTitle(job.status)).font(.headline)
                if !state.jobTitle.isEmpty { Text(state.jobTitle).font(.caption).foregroundStyle(.secondary) }
                ProgressView(value: Double(job.completed), total: Double(max(1, job.total)))
                Text("\(job.completed) / \(job.total) 件" + (job.rowID.isEmpty ? "" : " · \(job.rowID) · \(job.variantID)"))
                if job.active {
                    Button { Task { await operate { state.setJob(jsonObject(try await model.core.call("cancel_batch", ["jobId": job.id]))) } } } label: { Label("安全に停止する", systemImage: "stop.fill") }
                        .disabled(state.working || model.busy || job.status == "cancelling")
                } else if job.resumable {
                    Button { Task { await operate { state.setJob(jsonObject(try await model.core.call("resume_batch", ["jobId": job.id]))) } } } label: { Label("確認して再開する", systemImage: "arrow.clockwise") }
                        .disabled(state.working || model.busy)
                }
                NativeBatchIssueList(issues: job.issues)
            }.frame(maxWidth: .infinity, alignment: .leading)
        }
    }
    private var savedJobs: some View {
        GroupBox {
            HStack(alignment: .top, spacing: 14) {
                VStack(alignment: .leading, spacing: 6) {
                    Picker("保存済みジョブ", selection: $state.savedID) {
                        Text("ジョブを選択").tag("")
                        ForEach(state.saved) { item in Text("\(item.title) · revision \(item.revision) · \(item.total) 件").tag(item.id) }
                    }.disabled(disabled)
                    Text("再起動後は同じ出力先を選び直します。既存出力の内容を照合し、改変されたファイルには上書きしません。").font(.caption).foregroundStyle(.secondary)
                }
                Button("出力先を再選択して再開") { Task { await resumeSaved() } }.disabled(disabled || state.savedID.isEmpty)
                Button { Task { await refreshSaved() } } label: { Image(systemName: "arrow.clockwise") }.help("保存済みジョブを更新").disabled(state.working || model.busy)
            }
        } label: { Text("保存済みジョブから再開") }
    }
    private func invalidating<T>(_ key: ReferenceWritableKeyPath<NativeBatchState, T>) -> Binding<T> {
        Binding(get: { state[keyPath: key] }, set: { state[keyPath: key] = $0; state.plan = nil })
    }
    private func membership(_ value: String, in key: ReferenceWritableKeyPath<NativeBatchState, Set<String>>) -> Binding<Bool> {
        Binding(get: { state[keyPath: key].contains(value) }, set: { selected in
            if selected { state[keyPath: key].insert(value) } else { state[keyPath: key].remove(value) }; state.plan = nil
        })
    }
    private func operate(_ action: @escaping () async throws -> Void) async {
        guard !state.working else { return }; state.working = true; state.issues = []
        await model.perform {
            do { try await action() }
            catch { state.issues = (error as? StudioFailure)?.issues ?? [StudioIssue(["message": error.localizedDescription])]; throw error }
        }
        state.working = false
    }
    private func selectData() async {
        guard let current = session, let url = await model.pickFile(extensions: ["csv", "json"]) else { return }
        await operate {
            let raw = try await model.withAccess(url: url) { try await model.core.call("select_dataset_path", ["sessionId": current.sessionId, "path": url.path]) }
            if raw is NSNull { return }
            let dataset = NativeDataset(jsonObject(raw)); state.dataset = dataset; state.plan = nil
            let slots = Set(current.document.slots.map { jsonString($0["id"]) })
            state.mapping = Dictionary(uniqueKeysWithValues: dataset.columns.map { ($0, $0 == "rowId" || slots.contains($0) ? $0 : "") })
        }
    }
    private func inspect() async {
        guard canInspect, let current = session, let dataset = state.dataset, let url = await model.pickDirectory() else { return }
        await operate {
            let response = try await model.withAccess(url: url) {
                let latest = SessionView(jsonObject(try await model.core.call("read_session", ["sessionId": current.sessionId])))
                let variants = latest.document.variants.map { jsonString($0["id"]) }.filter { state.variants.contains($0) }
                let recipes: [[String: Any]] = allFormats.filter { state.formats.contains($0) }.map { format in
                    ["format": format, "quality": format == "jpeg" ? 90 : NSNull(), "jpegBackground": format == "jpeg" ? state.jpegBackground : NSNull(),
                     "webpLossless": true, "allowRasterization": state.rasterization, "pdfDpi": 144]
                }
                let raw = try await model.core.call("preflight_batch_path", ["sessionId": latest.sessionId, "revision": latest.document.revision,
                    "datasetId": dataset.id, "mapping": state.mapping.filter { !$0.value.isEmpty }, "variantIds": variants,
                    "namePattern": state.pattern, "errorPolicy": state.validOnly ? "valid-only" : "block-all", "recipes": recipes, "outputDirectory": url.path])
                if !(raw is NSNull) { state.retainGrant(url) }
                return raw
            }
            guard !(response is NSNull) else { return }
            let plan = NativeBatchPlan(jsonObject(response)); state.plan = plan; state.issues = plan.issues
            await refreshSaved()
        }
    }
    private func resumeSaved() async {
        let snapshotID = state.savedID
        guard !snapshotID.isEmpty, let url = await model.pickDirectory() else { return }
        await operate {
            let raw = try await model.withAccess(url: url) {
                let result = try await model.core.call("resume_saved_batch_path", ["snapshotId": snapshotID, "outputDirectory": url.path])
                if !(result is NSNull) { state.retainGrant(url) }
                return result
            }
            if !(raw is NSNull) { state.jobTitle = state.saved.first { $0.id == snapshotID }?.title ?? ""; state.setJob(jsonObject(raw)) }
        }
    }
    private func refreshSaved() async {
        do { state.saved = jsonObjects(try await model.core.call("list_saved_batches")).map(NativeSavedBatch.init) }
        catch { if !Task.isCancelled { state.issues = (error as? StudioFailure)?.issues ?? [StudioIssue(["message": error.localizedDescription])]; model.reportError(error) } }
    }
    private func pollJob() async {
        guard let id = state.job?.id, state.active else { return }
        while !Task.isCancelled, state.active, state.job?.id == id {
            do {
                let raw = try await model.core.call("batch_status", ["jobId": id])
                guard !Task.isCancelled, state.job?.id == id else { return }
                state.setJob(jsonObject(raw))
                if !state.active { await refreshSaved(); return }
                try await Task.sleep(nanoseconds: 600_000_000)
            } catch is CancellationError { return }
            catch { state.issues = (error as? StudioFailure)?.issues ?? [StudioIssue(["message": error.localizedDescription])]; model.reportError(error); return }
        }
    }
    private func statusTitle(_ status: String) -> String {
        ["queued": "待機中", "ready": "出力を準備中", "preflighting": "再開前の検査中", "running": "書き出し中", "cancelling": "安全に停止中",
         "completed": "完了", "cancelled": "停止しました", "interrupted": "中断しました", "failed": "失敗", "completed_with_errors": "問題のある候補を除いて完了"][status] ?? status
    }
}

@MainActor private struct NativeCandidateGallery: View {
    let model: StudioModel
    let planID: String
    let items: [NativeCandidate]
    @State private var page = 0
    @State private var images: [Int: Data] = [:]
    @State private var errors: [Int: String] = [:]
    private var pageCount: Int { max(1, (items.count + 11) / 12) }
    private var visibleItems: [NativeCandidate] { Array(items.dropFirst(page * 12).prefix(12)) }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("検査用縮小画像。各形式の元になる共通描画を表示します。JPEG背景などの形式固有の変換は書き出し結果に適用します。").font(.caption).foregroundStyle(.secondary)
            HStack {
                Button("前へ") { page -= 1 }.disabled(page == 0)
                Text("\(page + 1) / \(pageCount) ページ・全 \(items.count) 件").font(.caption)
                Button("次へ") { page += 1 }.disabled(page + 1 >= pageCount)
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 150, maximum: 220))], alignment: .leading, spacing: 12) {
                ForEach(visibleItems) { item in
                    VStack(alignment: .leading, spacing: 6) {
                        ZStack {
                            Rectangle().fill(Color(nsColor: .windowBackgroundColor))
                            if let data = images[item.id], let image = NSImage(data: data) { Image(nsImage: image).resizable().scaledToFit().accessibilityLabel("\(item.rowID) \(item.variantID) の検査画像") }
                            else { Text(item.issues.contains { $0.severity == "error" } ? "検査エラー" : errors[item.id] ?? "描画中…").font(.caption).foregroundStyle(.secondary).padding(8) }
                        }.frame(height: 145).clipShape(RoundedRectangle(cornerRadius: 6))
                        Text(item.file).font(.caption.weight(.semibold)).lineLimit(2).textSelection(.enabled)
                        Text("\(item.rowID) · \(item.variantID) · \(item.format.uppercased())").font(.caption2).foregroundStyle(.secondary)
                        if !item.issues.isEmpty { Text("\(item.issues.filter { $0.severity == "error" }.count) エラー / \(item.issues.filter { $0.severity == "warning" }.count) 注意").font(.caption2).foregroundStyle(.orange) }
                    }.padding(8).background(Color(nsColor: .controlBackgroundColor)).clipShape(RoundedRectangle(cornerRadius: 8))
                }
            }
        }
        .onChange(of: planID) { _ in page = 0; images = [:]; errors = [:] }
        .task(id: "\(planID)-\(page)") {
            images = [:]; errors = [:]
            // Only one request is submitted at a time. Changing pages cancels
            // the remaining work, bounding UI data to twelve thumbnails.
            for item in visibleItems {
                guard !Task.isCancelled else { return }
                if item.issues.contains(where: { $0.severity == "error" }) { continue }
                do {
                    let raw = jsonObject(try await model.core.call("native_batch_preview", ["planId": planID, "itemIndex": item.id]))
                    guard !Task.isCancelled else { return }
                    guard let data = Data(base64Encoded: jsonString(raw["pngBase64"])), data.count <= 2 * 1024 * 1024,
                          data.starts(with: [137, 80, 78, 71, 13, 10, 26, 10]) else { throw StudioFailure(issues: [StudioIssue(["message": "検査画像の形式が一致しません。"])] ) }
                    images[item.id] = data
                } catch { if !Task.isCancelled { errors[item.id] = error.localizedDescription } }
            }
        }
    }
}

@MainActor private struct NativeBatchIssueList: View {
    let issues: [StudioIssue]
    @State private var page = 0
    private var pageCount: Int { max(1, (issues.count + 19) / 20) }
    private var visibleIssues: [StudioIssue] { Array(issues.dropFirst(min(page, pageCount - 1) * 20).prefix(20)) }
    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if issues.count > 20 {
                HStack {
                    Button("前の問題") { page -= 1 }.disabled(page == 0)
                    Text("問題 \(min(page, pageCount - 1) + 1) / \(pageCount) ページ・全 \(issues.count) 件").font(.caption)
                    Button("次の問題") { page += 1 }.disabled(page + 1 >= pageCount)
                }
            }
            ForEach(visibleIssues) { issue in
                VStack(alignment: .leading, spacing: 4) {
                    Text("\(issue.code): \(issue.message)").font(.caption.weight(.semibold)).foregroundStyle(issue.severity == "error" ? Color.red : Color.orange)
                    let location = ["rowId", "variantId", "exportFormat", "nodeId", "assetId", "fontId"].compactMap { key -> String? in
                        let value = jsonString(issue.location[key]); return value.isEmpty ? nil : "\(key): \(value)"
                    }.joined(separator: " · ")
                    if !location.isEmpty { Text(location).font(.caption2).foregroundStyle(.secondary).textSelection(.enabled) }
                    if !issue.remedy.isEmpty { Text(issue.remedy).font(.caption).foregroundStyle(.secondary) }
                }.textSelection(.enabled)
            }
        }.onChange(of: issues.map(\.id)) { _ in page = 0 }
    }
}
