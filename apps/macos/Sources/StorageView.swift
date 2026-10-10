import Foundation
import SwiftUI

private struct NativeStorageStatus {
    let path: String
    let workspaceBytes: Double
    let recoveryBytes: Double
    let jobBytes: Double
    let cleanWorkspaceBytes: Double
    let cleanRecoveryBytes: Double
    let cleanJobBytes: Double
    let protectedCount: Int
    init(_ raw: [String: Any]) {
        path = jsonString(raw["displayPath"])
        workspaceBytes = jsonNumber(raw["workspaceBytes"])
        recoveryBytes = jsonNumber(raw["recoveryBytes"])
        jobBytes = jsonNumber(raw["jobBytes"])
        cleanWorkspaceBytes = jsonNumber(raw["cleanableWorkspaceBytes"])
        cleanRecoveryBytes = jsonNumber(raw["cleanableRecoveryBytes"])
        cleanJobBytes = jsonNumber(raw["cleanableJobBytes"])
        protectedCount = Int(jsonNumber(raw["protectedCount"]))
    }
}
private struct NativeStorageCategory: Identifiable {
    let id: String
    let name: String
    let total: Double
    let cleanable: Double
    let description: String
}

@MainActor struct StorageView: View {
    @ObservedObject var model: StudioModel
    @Environment(\.dismiss) private var dismiss
    @State private var status: NativeStorageStatus?
    @State private var working = false
    @State private var confirmation: NativeStorageCategory?
    @State private var showConfirmation = false
    @State private var notice = ""
    @State private var failures: [StudioIssue] = []
    private var categories: [NativeStorageCategory] {
        guard let status else { return [] }
        return [
            NativeStorageCategory(id: "workspaces", name: "ローカル作業領域", total: status.workspaceBytes,
                cleanable: status.cleanWorkspaceBytes, description: "保存・復旧・ジョブから参照されていない、閉じた作品の一時データ。"),
            NativeStorageCategory(id: "recovery", name: "編集の復旧データ", total: status.recoveryBytes,
                cleanable: status.cleanRecoveryBytes, description: "閉じた作品の古い復旧データ。削除すると、その編集を復元できなくなります。"),
            NativeStorageCategory(id: "jobs", name: "量産ジョブの保存データ", total: status.jobBytes,
                cleanable: status.cleanJobBytes, description: "完了済みジョブの入力スナップショット。書き出した成果物には影響しません。")
        ]
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack {
                Text("この Mac 内の保存領域").font(.title2.weight(.semibold))
                Spacer()
                if working { ProgressView().controlSize(.small) }
                Button("閉じる") { dismiss() }.keyboardShortcut(.cancelAction).disabled(working)
            }
            Label("開いている作品、最新の復旧、実行中・中断中のジョブは保護します。", systemImage: "checkmark.shield")
                .font(.callout).foregroundStyle(.secondary)
            if let status {
                if !status.path.isEmpty { Text(status.path).font(.caption).textSelection(.enabled).foregroundStyle(.secondary) }
                Text("\(status.protectedCount) 件の作業領域を保護中").font(.caption).foregroundStyle(.secondary)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    ForEach(categories) { category in
                        GroupBox {
                            HStack(alignment: .top, spacing: 14) {
                                Image(systemName: "internaldrive").font(.title3).foregroundStyle(.secondary)
                                VStack(alignment: .leading, spacing: 7) {
                                    Text(category.name).font(.headline)
                                    Text("\(size(category.total)) · 整理可能 \(size(category.cleanable))").font(.callout)
                                    Text(category.description).font(.caption).foregroundStyle(.secondary)
                                }
                                Spacer()
                                Button("整理する") { confirmation = category; showConfirmation = true }
                                    .disabled(working || model.busy || category.cleanable <= 0)
                                    .accessibilityLabel("\(category.name)を整理する")
                            }.frame(maxWidth: .infinity, alignment: .leading).padding(5)
                        }
                    }
                    ForEach(failures) { issue in
                        VStack(alignment: .leading, spacing: 4) {
                            Text("\(issue.code): \(issue.message)").font(.caption.weight(.semibold)).foregroundStyle(.red)
                            if !issue.remedy.isEmpty { Text(issue.remedy).font(.caption).foregroundStyle(.secondary) }
                        }.textSelection(.enabled)
                    }
                }
            }
            HStack {
                Button { Task { await refresh() } } label: { Label("使用量を更新", systemImage: "arrow.clockwise") }
                    .disabled(working || model.busy)
                if !notice.isEmpty { Text(notice).font(.caption).foregroundStyle(.secondary).accessibilityLabel(notice) }
            }
            Text("保存済みプロジェクトと出力先のファイルを、この設定から削除することはありません。").font(.caption).foregroundStyle(.secondary)
        }
        .padding(24).frame(minWidth: 620, idealWidth: 700, minHeight: 540, idealHeight: 650)
        .interactiveDismissDisabled(working)
        .task { await refresh() }
        .confirmationDialog("\(confirmation?.name ?? "保存領域")を削除しますか？", isPresented: $showConfirmation, titleVisibility: .visible) {
            Button("確認して削除する", role: .destructive) {
                guard let selected = confirmation else { return }
                Task { await clean(selected) }
            }
            Button("キャンセル", role: .cancel) { confirmation = nil }
        } message: {
            Text("\(confirmation?.description ?? "") 整理可能なデータだけを削除します。この削除は元に戻せません。")
        }
    }
    private func size(_ bytes: Double) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(max(0, min(bytes, Double(Int64.max - 1024)))), countStyle: .file)
    }
    private func refresh() async {
        guard !working else { return }; working = true; failures = []
        defer { working = false }
        do { status = NativeStorageStatus(jsonObject(try await model.core.call("storage_status"))) }
        catch { failures = (error as? StudioFailure)?.issues ?? [StudioIssue(["message": error.localizedDescription])]; model.reportError(error) }
    }
    private func clean(_ category: NativeStorageCategory) async {
        guard !working, category.cleanable > 0 else { return }
        working = true; failures = []; notice = ""
        // This function is reached only by the destructive confirmation button.
        // Native cleanup rechecks live sessions/jobs and protected recovery data.
        await model.perform {
            do {
                let result = jsonObject(try await model.core.call("cleanup_storage", ["kind": category.id]))
                status = NativeStorageStatus(jsonObject(result["status"]))
                notice = "\(Int(jsonNumber(result["removedCount"]))) 件・\(size(jsonNumber(result["removedBytes"])))を整理しました"
                confirmation = nil
            } catch { failures = (error as? StudioFailure)?.issues ?? [StudioIssue(["message": error.localizedDescription])]; throw error }
        }
        working = false
    }
}
