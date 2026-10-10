import AppKit
import Foundation
import SwiftUI
import UniformTypeIdentifiers

func jsonObject(_ value: Any?) -> [String: Any] { value as? [String: Any] ?? [:] }
func jsonObjects(_ value: Any?) -> [[String: Any]] { value as? [[String: Any]] ?? [] }
func jsonString(_ value: Any?, fallback: String = "") -> String { value as? String ?? fallback }
func jsonNumber(_ value: Any?, fallback: Double = 0) -> Double { (value as? NSNumber)?.doubleValue ?? fallback }
func jsonBool(_ value: Any?, fallback: Bool = false) -> Bool { value as? Bool ?? fallback }

struct StudioIssue: Identifiable {
    let id = UUID()
    var severity: String
    var code: String
    var message: String
    var remedy: String
    var location: [String: Any]
    init(_ raw: [String: Any]) {
        severity = jsonString(raw["severity"], fallback: "error")
        code = jsonString(raw["code"], fallback: "E_OPERATION")
        message = jsonString(raw["message"])
        remedy = jsonString(raw["remedy"])
        location = jsonObject(raw["location"])
    }
}
struct StudioFailure: Error, LocalizedError {
    let issues: [StudioIssue]
    var errorDescription: String? { issues.first?.message }
}
struct Document {
    var raw: [String: Any]
    var title: String { jsonString(raw["title"]) }
    var revision: UInt64 { (raw["revision"] as? NSNumber)?.uint64Value ?? 0 }
    var nodes: [[String: Any]] { jsonObjects(raw["nodes"]) }
    var variants: [[String: Any]] { jsonObjects(raw["variants"]) }
    var slots: [[String: Any]] { jsonObjects(raw["slots"]) }
    var assets: [[String: Any]] { jsonObjects(raw["assets"]) }
    var fonts: [[String: Any]] { jsonObjects(raw["fonts"]) }
    var nodeStyles: [[String: Any]] { jsonObjects(raw["nodeStyles"]) }
    var brandSnapshot: [String: Any]? { raw["brandSnapshot"] as? [String: Any] }
    func node(_ id: String) -> [String: Any]? { nodes.first { jsonString($0["id"]) == id } }
    func variant(_ id: String) -> [String: Any]? { variants.first { jsonString($0["id"]) == id } }
}
struct SessionView {
    var sessionId: String
    var document: Document
    var mode: String
    var canUndo: Bool
    var canRedo: Bool
    var issues: [StudioIssue]
    init(_ raw: [String: Any]) {
        sessionId = jsonString(raw["sessionId"])
        document = Document(raw: jsonObject(raw["document"]))
        mode = jsonString(raw["mode"], fallback: "authoring")
        canUndo = jsonBool(raw["canUndo"])
        canRedo = jsonBool(raw["canRedo"])
        issues = jsonObjects(raw["issues"]).map(StudioIssue.init)
    }
}

@MainActor final class StudioModel: ObservableObject {
    @Published var session: SessionView?
    @Published var selected: [String] = []
    @Published var variantID = "portrait"
    @Published var busy = false
    @Published var issues: [StudioIssue] = []
    @Published var preview: Data?
    @Published var previewLoading = false
    @Published var notice = ""
    @Published var showTemplates = false
    @Published var showExport = false
    @Published var showBatch = false
    @Published var showStorage = false
    @Published var savedRevision: UInt64?
    let core = CoreClient()
    private var queue: Task<Void, Never>?
    private var pending = 0
    private var choosingFile = false
    private var nextPreview: UInt64 = 0
    private var previewIssues: [StudioIssue] = []
    private var operationIssues: [StudioIssue] = []
    private var started = false
    var authoring: Bool { session?.mode == "authoring" }
    var variant: [String: Any]? { session?.document.variant(variantID) }

    func commitFocusedEditing() async -> Bool {
        if let text=NSApp.keyWindow?.firstResponder as? NSTextView, text.hasMarkedText() {
            notice="文字の変換を確定してから操作してください"
            return false
        }
        NSApp.keyWindow?.makeFirstResponder(nil)
        await Task.yield()
        return true
    }

    func updateIssues() { issues = operationIssues + previewIssues }
    func reportError(_ error: Error) {
        operationIssues = (error as? StudioFailure)?.issues ?? [StudioIssue(["message": error.localizedDescription, "remedy": "内容を確認し、もう一度お試しください。"])]
        updateIssues()
    }
    func perform(_ operation: @escaping () async throws -> Void) async {
        let prior = queue
        pending += 1; busy = true
        let next = Task { @MainActor in
            await prior?.value
            do { try await operation() }
            catch {
                let failure = error
                if let id = self.session?.sessionId,
                   let raw = try? await self.core.call("read_session", ["sessionId": id]) {
                    self.accept(jsonObject(raw))
                }
                self.reportError(failure)
            }
            self.pending -= 1; self.busy = self.pending > 0 || self.choosingFile
        }
        queue = next
        await next.value
    }
    func accept(_ raw: [String: Any], reset: Bool = false) {
        let view = SessionView(raw)
        guard !view.sessionId.isEmpty else { return }
        let previous = session
        session = view
        if view.document.variant(variantID) == nil { variantID = jsonString(view.document.variants.first?["id"]) }
        selected = reset ? [] : selected.filter { view.document.node($0) != nil }
        operationIssues = view.issues
        if previous?.sessionId != view.sessionId || previous?.document.revision != view.document.revision {
            preview = nil; previewIssues = []
        }
        updateIssues()
        if let old = previous?.sessionId, old != view.sessionId {
            Task { _ = try? await core.call("close_session", ["sessionId": old]) }
        }
        Task { await refreshPreview() }
    }
    func start() async {
        guard !started else { return }; started = true
        await perform {
            do { self.accept(jsonObject(try await self.core.call("recover_project")), reset: true); self.notice = "前回の確定した編集を復旧しました" }
            catch {
                let failures = (error as? StudioFailure)?.issues ?? []
                if failures.count == 1 && failures[0].message == "no recoverable document has been committed" {
                    self.accept(jsonObject(try await self.core.call("open_fixture", ["fixture":"retail-promo-template"])), reset: true)
                    self.showTemplates = true
                } else { throw error }
            }
        }
    }
    func openTemplate(_ id: String) async {
        guard await commitFocusedEditing() else{return}
        await perform { self.accept(jsonObject(try await self.core.call("open_fixture", ["fixture":id])), reset:true); self.savedRevision=nil; self.showTemplates=false }
    }
    func newDocument() async {
        guard await commitFocusedEditing() else{return}
        await perform { self.accept(jsonObject(try await self.core.call("new_document")),reset:true);self.savedRevision=nil }
    }
    func setVariant(_ id: String) async {guard await commitFocusedEditing() else{return};variantID=id;selected=[];await refreshPreview() }
    func refreshPreview() async {
        guard let current=session, let variant=current.document.variant(variantID) else { return }
        nextPreview += 1
        let request=nextPreview,revision=current.document.revision,id=variantID
        previewLoading=true
        do {
            let raw=jsonObject(try await core.call("native_preview",["sessionId":current.sessionId,"revision":revision,"variantId":id,"requestId":request]))
            guard request==nextPreview,session?.sessionId==current.sessionId,session?.document.revision==revision,variantID==id else {return}
            let meta=jsonObject(raw["metadata"])
            guard jsonNumber(meta["width"])==jsonNumber(variant["width"]),jsonNumber(meta["height"])==jsonNumber(variant["height"]),let data=Data(base64Encoded:jsonString(raw["pngBase64"])) else { throw StudioFailure(issues:[StudioIssue(["message":"プレビューの形式または寸法が一致しません。"])] ) }
            preview=data;previewIssues=jsonObjects(meta["issues"]).map(StudioIssue.init);updateIssues()
        } catch { if request==nextPreview {previewIssues=(error as? StudioFailure)?.issues ?? [StudioIssue(["message":error.localizedDescription])];updateIssues()} }
        if request==nextPreview {previewLoading=false}
    }
    func edit(_ command: [String: Any], context: EditorCommitContext? = nil) async {
        await perform {
            guard let current=self.session else{return}
            guard context?.matches(sessionID:current.sessionId,variantID:self.variantID) != false else{return}
            var active=command
            let style=jsonObject(command["style"]),nodeID=jsonString(command["nodeId"])
            if jsonString(command["op"])=="setTextStyle",style["fontSize"] != nil || style["lineHeight"] != nil,
               var variant=current.document.variant(self.variantID) {
                var patches=jsonObjects(variant["nodePatches"])
                if let index=patches.firstIndex(where:{jsonString($0["nodeId"])==nodeID}) {
                    if let size=style["fontSize"] {patches[index]["fontSize"]=size}
                    if let line=style["lineHeight"] {patches[index]["lineHeight"]=line}
                    variant["nodePatches"]=patches;variant["approvalStatus"]="draft"
                    active=["op":"batch","commands":[command,["op":"setVariant","variant":variant]]]
                }
            }
            self.accept(jsonObject(try await self.core.call("apply_edit",["sessionId":current.sessionId,"expectedRevision":current.document.revision,"transactionId":UUID().uuidString,"command":active])))
        }
    }
    func changeBounds(changes: [[String: Any]]) async {
        guard let current=session else{return}
        await edit(EditorGeometry.boundsCommand(current.document.raw,variantID:variantID,changes:changes))
    }
    func duplicateSelection() async {
        guard let current=session,let copy=EditorGeometry.duplicateCommand(current.document.raw,selected:selected) else{return}
        await edit(copy.command);selected=copy.selected
    }
    func deleteSelection() async {
        guard let current=session else{return}
        let ids=selected.filter { id in !EditorGeometry.ancestorIDs(current.document.nodes,id:id).contains(where:selected.contains) }
        await edit(["op":"batch","commands":ids.map{["op":"deleteNode","nodeId":$0]}])
    }
    func history(_ direction: String) async {
        if let text=NSApp.keyWindow?.firstResponder as? NSTextView {
            guard !text.hasMarkedText() else{return}
            if direction=="undo",text.undoManager?.canUndo==true {text.undoManager?.undo();return}
            if direction=="redo",text.undoManager?.canRedo==true {text.undoManager?.redo();return}
        }
        guard await commitFocusedEditing() else{return}
        await perform {guard let current=self.session,direction=="undo" ? current.canUndo:current.canRedo else{return};self.accept(jsonObject(try await self.core.call("\(direction)_edit",["sessionId":current.sessionId,"expectedRevision":current.document.revision,"transactionId":UUID().uuidString]))) }
    }
    func setMode(_ mode:String) async {
        guard await commitFocusedEditing() else{return}
        await perform {guard let current=self.session else{return};self.accept(jsonObject(try await self.core.call("set_mode",["sessionId":current.sessionId,"expectedRevision":current.document.revision,"transactionId":UUID().uuidString,"mode":mode]))) }
    }
    func beginExport() async {guard await commitFocusedEditing() else{return};showExport=true}
    func beginBatch() async {guard await commitFocusedEditing() else{return};showBatch=true}
    private func chooseURL(_ panel:NSSavePanel) async -> URL? {
        guard !choosingFile else{return nil}
        choosingFile=true;busy=true
        defer {choosingFile=false;busy=pending>0}
        return await withCheckedContinuation { continuation in
            NSLog("SILIQ picker start")
            panel.begin { response in
                NSLog("SILIQ picker response %ld hasURL %d",response.rawValue,panel.url == nil ? 0:1)
                continuation.resume(returning:response == .OK ? panel.url:nil)
            }
        }
    }
    func pickFile(extensions: [String]) async -> URL? {
        let panel=NSOpenPanel();panel.canChooseDirectories=false;panel.canChooseFiles=true;panel.allowsMultipleSelection=false
        panel.allowedContentTypes=extensions.compactMap{UTType(filenameExtension:$0)}
        return await chooseURL(panel)
    }
    func pickDirectory() async -> URL? {
        let panel=NSOpenPanel();panel.canChooseDirectories=true;panel.canChooseFiles=false;panel.allowsMultipleSelection=false
        return await chooseURL(panel)
    }
    func saveFile(name: String, extension fileExtension: String) async -> URL? {
        let panel=NSSavePanel();panel.allowedContentTypes=[UTType(filenameExtension:fileExtension) ?? .data];panel.nameFieldStringValue=name
        return await chooseURL(panel)
    }
    func withAccess<T>(url:URL,operation:() async throws -> T) async rethrows -> T {
        let scoped=url.startAccessingSecurityScopedResource();defer {if scoped{url.stopAccessingSecurityScopedResource()}}
        return try await operation()
    }
    func openProject() async {
        guard await commitFocusedEditing() else{return}
        guard let url=await pickFile(extensions:["siliqcg"]) else{return}
        await perform {try await self.withAccess(url:url){let raw=jsonObject(try await self.core.call("open_project_path",["path":url.path]));self.accept(raw,reset:true);self.savedRevision=self.session?.document.revision}}
    }
    func saveProject(saveAs: Bool = false) async {
        guard await commitFocusedEditing() else{return}
        guard let current=session,let url=await saveFile(name:"\(current.document.title).siliqcg",extension:"siliqcg") else{return}
        await perform {guard let now=self.session else{return};try await self.withAccess(url:url){let overwrite=FileManager.default.fileExists(atPath:url.path);_ = try await self.core.call("save_project_path",["sessionId":now.sessionId,"expectedRevision":now.document.revision,"path":url.path,"overwrite":overwrite]);self.savedRevision=now.document.revision;self.notice="プロジェクトを保存しました"}}
    }
    func importAsset(_ kind:String,addToCanvas:Bool=false) async {
        guard await commitFocusedEditing() else{return}
        let extensions=kind=="font" ? ["otf","ttf","ttc"] : kind=="svg" ? ["svg"] : ["png","jpg","jpeg","webp"]
        guard let url=await pickFile(extensions:extensions) else{return}
        await perform {
            guard let current=self.session else{return}
            let imported = try await self.withAccess(url:url){jsonObject(try await self.core.call("import_asset_path",["sessionId":current.sessionId,"expectedRevision":current.document.revision,"transactionId":UUID().uuidString,"kind":kind,"path":url.path]))}
            let importedID=jsonString(imported["importedResourceId"])
            self.accept(imported)
            if addToCanvas,kind != "font",let now=self.session,let asset=now.document.assets.first(where:{jsonString($0["id"])==importedID}) {
                let id="media-\(UUID().uuidString.lowercased().prefix(8))",root=jsonString(jsonObject(now.document.raw["baseArtboard"])["rootNodeId"])
                var node:[String:Any]=["id":id,"name":kind=="svg" ? "ロゴ":"画像","type":kind=="svg" ? "logo":"image","bounds":["x":80,"y":100,"width":500,"height":340],"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"assetId":jsonString(asset["id"]),"fit":"contain"]
                if kind=="svg" {node["preserveAspectRatio"]=true;node["minWidth"]=1;node["clearSpace"]=0} else {node["focalPoint"]=["x":0.5,"y":0.5];node["clipToBounds"]=true}
                let command:[String:Any]=["op":"addNode","parentId":root,"index":(now.document.node(root)?["childIds"] as? [String] ?? []).count,"node":node]
                self.accept(jsonObject(try await self.core.call("apply_edit",["sessionId":now.sessionId,"expectedRevision":now.document.revision,"transactionId":UUID().uuidString,"command":command])));self.selected=[id]
            }
        }
    }
    func resolveFont(_ fontID:String) async {
        guard let url=await pickFile(extensions:["otf","ttf","ttc"]) else{return}
        await perform {guard let current=self.session else{return};try await self.withAccess(url:url){self.accept(jsonObject(try await self.core.call("resolve_font_path",["sessionId":current.sessionId,"expectedRevision":current.document.revision,"transactionId":UUID().uuidString,"fontId":fontID,"path":url.path])))}}
    }
    func addNode(_ type:String) async {
        guard let current=session else{return}
        let id="\(type)-\(UUID().uuidString.lowercased().prefix(8))",root=jsonString(jsonObject(current.document.raw["baseArtboard"])["rootNodeId"])
        var node:[String:Any]=["id":id,"name":type=="text" ? "テキスト":type=="path" ? "ベジェパス":"図形","type":type,"bounds":["x":80,"y":100,"width":500,"height":type=="text" ? 100:240],"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"fill":["kind":"solid","color":type=="text" ? "#202723":"#39735b"]]
        if type=="text" {node["text"]=["value":"ここにテキストを入力","fontId":jsonString(current.document.fonts.first?["id"]),"fontSize":40,"lineHeight":52,"letterSpacing":0,"maxLines":2,"overflowPolicy":"error","allowedFontSizes":[],"language":"ja","align":"left"]}
        if type=="rect" {node["cornerRadius"]=16}
        if type=="path" {node["data"]="M 0 180 C 100 20 300 20 500 180 L 500 240 L 0 240 Z";node["viewBox"]=["x":0,"y":0,"width":500,"height":240];node["fillRule"]="non-zero"}
        let add:[String:Any]=["op":"addNode","parentId":root,"index":(current.document.node(root)?["childIds"] as? [String] ?? []).count,"node":node]
        await edit(type=="text" ? ["op":"batch","commands":[add,["op":"setTextStyle","nodeId":id,"style":["kinsoku":"strict-japanese"]]]] : add);selected=[id]
    }
}
