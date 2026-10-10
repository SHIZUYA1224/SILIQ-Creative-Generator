import AppKit
import Foundation

// Direct AppKit input-client checks. No window is created or shown, no key
// events are injected, and the Rust broker is never instantiated. These do not
// replace hardware keyboard / selected input-method / reconversion tests.
@main struct EditorChecks {
    enum Failure: Error, CustomStringConvertible {
        case assertion(String)
        var description:String { if case .assertion(let message)=self { return message };return "check failed" }
    }
    @MainActor static func main() throws {
        let root=CommandLine.arguments.dropFirst().first ?? FileManager.default.currentDirectoryPath
        let fixture=URL(fileURLWithPath:root).appendingPathComponent("fixtures/public-safe/documents/basic.json")
        let raw=try JSONSerialization.jsonObject(with:Data(contentsOf:fixture)) as! [String:Any]
        try geometry(raw)
        try composition()
        print("Native editor: 16 checks passed (10 geometry/context/path, 6 direct AppKit composition)")
        print("No visible window or keyboard automation; hardware IME/reconversion remains a separate release check.")
    }
    static func require(_ condition:@autoclosure ()->Bool,_ message:String) throws { if !condition() { throw Failure.assertion(message) } }

    static func geometry(_ document:[String:Any]) throws {
        let inputContext=EditorCommitContext(sessionID:"document-a",variantID:"square",nodeID:"headline")
        let textCommand=inputContext.nodeCommand("setText",fields:["value":"A の確定した編集","nodeId":"different-selected-node"])
        let geometryCommand=inputContext.nodeCommand("setBounds",fields:["bounds":["x":12,"y":34,"width":56,"height":78]])
        try require(EditorJSON.string(textCommand["nodeId"])=="headline" && EditorJSON.string(geometryCommand["nodeId"])=="headline","staged text or numeric input changed its original node target")
        try require(inputContext.matches(sessionID:"document-a",variantID:"square"),"changing layer selection should preserve the original input target")
        try require(!inputContext.matches(sessionID:"document-b",variantID:"square") && !inputContext.matches(sessionID:"document-a",variantID:"portrait"),"old input can cross a document or artboard switch")

        let portrait=EditorGeometry.resolvedNodes(document,variantID:"portrait")
        let headline=portrait.first { EditorJSON.string($0["id"])=="headline" }!
        try require(EditorJSON.number(EditorJSON.object(headline["text"])["fontSize"])==60,"variant text override was not applied")
        try require(EditorJSON.number(EditorJSON.object(EditorJSON.objects(document["nodes"]).first { EditorJSON.string($0["id"])=="headline" }!["text"])["fontSize"])==64,"variant resolution changed canonical text")

        var nodes=EditorJSON.objects(document["nodes"])
        let parent=nodes.firstIndex { EditorJSON.string($0["id"])=="accent-group" }!
        nodes[parent]["rotationDeg"]=90
        let child=nodes.first { EditorJSON.string($0["id"])=="accent" }!
        let styles:[[String:Any]]=[["nodeId":"accent-group","affine":[2,0,0,2,0,0]]]
        let movement=EditorGeometry.parentTransform(nodes,node:child,styles:styles).inverseVector(dx:0,dy:20)
        try require(abs(movement.dx-10)<1e-9 && abs(movement.dy)<1e-9,"parent inverse does not preserve world movement")

        let frame=EditorGeometry.frame(nodes,node:nodes[parent],styles:styles)
        try require(abs(hypot(frame.a,frame.b)-1)<1e-9,"group frame applies its own affine twice")

        var transformed=document
        transformed["nodes"]=nodes;transformed["nodeStyles"]=styles
        var variants=EditorJSON.objects(transformed["variants"])
        let index=variants.firstIndex { EditorJSON.string($0["id"])=="portrait" }!
        var patches=EditorJSON.objects(variants[index]["nodePatches"])
        let before=EditorBounds(nodes[parent]["bounds"])
        patches.removeAll { EditorJSON.string($0["nodeId"])=="accent-group" }
        patches.append(["nodeId":"accent-group","bounds":before.raw,"affine":[3,0,0,3,5,6]])
        variants[index]["nodePatches"]=patches;transformed["variants"]=variants
        let command=EditorGeometry.boundsCommand(transformed,variantID:"portrait",changes:[["id":"accent-group","bounds":before.moved(20,30).raw]])
        let patch=EditorJSON.objects(EditorJSON.object(command["variant"])["nodePatches"]).first { EditorJSON.string($0["nodeId"])=="accent-group" }!
        try require(EditorMatrix(patch["affine"]).raw==[3,0,0,3,25,36],"variant group affine was lost or computed from the wrong base")

        let copy=EditorGeometry.duplicateCommand(transformed,selected:["accent-group","accent"])!
        let adds=EditorJSON.objects(copy.command["commands"]).filter { EditorJSON.string($0["op"])=="addNode" }
        try require(copy.selected.count==1 && adds.count==2,"ancestor and child were duplicated twice")
        try require(EditorJSON.string(adds[0]["parentId"])=="root" && EditorJSON.string(adds[1]["parentId"])==copy.selected[0],"subtree duplication changed parents")
        try require(EditorJSON.equal(EditorJSON.object(EditorJSON.object(adds[1]["node"])["bounds"]),EditorJSON.object(child["bounds"])),"copy moved descendants independently")

        let path=try EditorPath("m1 2 c3 4 5 6 7 8 s1 2 3 4 q1 1 2 2 t3 3 z")
        try require(path.segments.map(\.command)==["M","C","C","Q","Q","Z"],"relative/smooth curves did not expand")
        let again=try EditorPath(path.serialized())
        try require(again.segments[2].points[0]==path.segments[2].points[0],"canonical serialization changed a control point")

        var arcRejected=false
        do { _=try EditorPath("M0 0 A5 5 0 0 0 10 10") } catch { arcRejected=true }
        try require(arcRejected,"unsupported arc was silently accepted")

        var boundsRejected=false
        do { _=try EditorPath("M999999 0 l2 0") } catch { boundsRejected=true }
        try require(boundsRejected,"expanded relative coordinates escaped the finite bounds")
        print("Geometry/context/path: 10 checks passed")
    }

    @MainActor static func composition() throws {
        let application=NSApplication.shared
        application.setActivationPolicy(.prohibited)
        let text=ConfirmedTextView(frame:NSRect(x:0,y:0,width:400,height:200))
        text.isRichText=false;text.isEditable=true;text.multiline=true
        text.string="元の文章";text.committed=text.string;text.source=text.string
        var commits:[String]=[];text.onCommit={commits.append($0)}

        text.setMarkedText("にほんご",selectedRange:NSRange(location:4,length:0),replacementRange:NSRange(location:0,length:text.string.utf16.count))
        try require(text.hasMarkedText(),"AppKit did not retain marked text")
        text.commitConfirmed();try require(commits.isEmpty,"marked text became an edit transaction")

        text.insertText("日本語",replacementRange:NSRange(location:NSNotFound,length:0))
        text.commitConfirmed();text.commitConfirmed()
        try require(commits==["日本語"],"confirmed text was lost or committed more than once")

        text.setMarkedText("がく",selectedRange:NSRange(location:2,length:0),replacementRange:NSRange(location:0,length:text.string.utf16.count))
        text.pendingBlur=true
        text.insertText("学ぶ",replacementRange:NSRange(location:NSNotFound,length:0));text.commitConfirmed()
        try require(commits==["日本語","学ぶ"],"confirmation after focus loss was not committed once")

        text.setMarkedText("キャンセル",selectedRange:NSRange(location:5,length:0),replacementRange:NSRange(location:0,length:text.string.utf16.count))
        text.insertText("学ぶ",replacementRange:NSRange(location:NSNotFound,length:0));text.commitConfirmed()
        try require(commits.count==2,"cancelled composition added a transaction")

        text.string="が";text.commitConfirmed()
        let decomposed="か\u{3099}"
        try require(decomposed=="が" && !decomposed.utf8.elementsEqual("が".utf8),"combining fixture must have distinct authored UTF-8 bytes")
        text.string=decomposed;text.commitConfirmed()
        try require(commits.count==4 && commits.last!.utf8.elementsEqual(decomposed.utf8),"canonical-equivalent text lost the authored UTF-8 representation")
        text.string="😀 mixed 日本語 001";text.commitConfirmed()
        try require(commits.last!.utf8.elementsEqual("😀 mixed 日本語 001".utf8),"mixed/astral characters changed")

        let original=EditorCommitContext(sessionID:"document-a",variantID:"square",nodeID:"headline")
        var activeSession="document-a",activeVariant="square",selectedNode="headline"
        var routed:[[String:Any]]=[]
        text.onCommit={ value in
            guard original.matches(sessionID:activeSession,variantID:activeVariant) else{return}
            routed.append(original.nodeCommand("setText",fields:["value":value]))
        }
        selectedNode="caption";text.string="先のレイヤーに保持する本文";text.commitConfirmed()
        try require(selectedNode=="caption" && routed.count==1 && EditorJSON.string(routed[0]["nodeId"])=="headline","blur after layer selection wrote to the newly selected layer")
        activeVariant="portrait";text.string="旧アートボードの入力";text.commitConfirmed()
        activeVariant="square";activeSession="document-b";text.string="旧作品の入力";text.commitConfirmed()
        try require(routed.count==1,"a stale AppKit callback wrote into another artboard or document")
        print("Direct AppKit composition: 6 checks passed")
    }
}
