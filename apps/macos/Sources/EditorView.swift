import SwiftUI
import AppKit

private struct EditorLayer: Identifiable {
    var node:[String:Any];var depth:Int
    var id:String { EditorJSON.string(node["id"]) }
}

struct EditorView: View {
    @ObservedObject var model:StudioModel
    @State private var zoom=1.0
    @State private var grid=false
    @State private var safe=false
    @State private var snap=true
    @State private var guides=false
    @State private var guideX=50.0
    @State private var guideY=50.0
    private var document:[String:Any] { model.session?.document.raw ?? [:] }
    private var nodes:[[String:Any]] { EditorGeometry.resolvedNodes(document,variantID:model.variantID) }
    private var styles:[[String:Any]] { EditorGeometry.resolvedStyles(document,variantID:model.variantID) }
    private var variant:[String:Any] { model.session?.document.variant(model.variantID) ?? model.session?.document.variants.first ?? [:] }
    private var rootID:String { EditorJSON.string(EditorJSON.object(document["baseArtboard"])["rootNodeId"]) }
    private var layers:[EditorLayer] { EditorGeometry.tree(nodes,rootID:rootID).map { EditorLayer(node:$0.node,depth:$0.depth) } }
    private var authoring:Bool { model.session?.mode == "authoring" }

    var body: some View {
        HStack(spacing:0) {
            sidebar.frame(width:220)
            Divider()
            center.frame(minWidth:360,maxWidth:.infinity,maxHeight:.infinity)
            Divider()
            InspectorView(model:model).frame(width:320)
        }
    }
    private var sidebar: some View {
        VStack(alignment:.leading,spacing:0) {
            Text("アートボード").font(.headline).padding()
            ForEach((model.session?.document.variants ?? []).map { EditorLayer(node:$0,depth:0) }) { item in
                let variant=item.node
                Button { Task { await model.setVariant(EditorJSON.string(variant["id"])) } } label: {
                    HStack {
                        Image(systemName:"rectangle")
                        VStack(alignment:.leading) { Text(EditorJSON.string(variant["label"])).lineLimit(1);Text("\(Int(EditorJSON.number(variant["width"]))) × \(Int(EditorJSON.number(variant["height"])))").font(.caption).foregroundStyle(.secondary) }
                        Spacer();Circle().fill(EditorJSON.string(variant["approvalStatus"])=="approved" ? Color.green:Color.orange).frame(width:7,height:7)
                    }.padding(10).background(EditorJSON.string(variant["id"])==model.variantID ? Color.accentColor.opacity(0.12):Color.clear)
                }.buttonStyle(.plain).disabled(model.busy)
            }
            Divider().padding(.vertical,8)
            HStack { Text("レイヤー").font(.headline);Spacer();Text("\(layers.count)").foregroundStyle(.secondary) }.padding(.horizontal)
            HStack(spacing:12) {
                iconButton("グループ化","square.stack.3d.up",disabled:!authoring || model.busy || model.selected.count<2) { Task { await model.edit(["op":"groupNodes","nodeIds":model.selected,"groupId":"group-"+UUID().uuidString.lowercased().prefix(8),"name":"グループ"]) } }
                iconButton("グループを解除","rectangle.3.group",disabled:!authoring || model.busy || EditorJSON.string(nodes.first { EditorJSON.string($0["id"])==model.selected.first }?["type"]) != "group") { if let id=model.selected.first { Task { await model.edit(["op":"ungroupNodes","groupId":id]) } } }
                iconButton("前面へ","arrow.up",disabled:!authoring || model.busy || model.selected.count != 1) { reorder(1) }
                iconButton("背面へ","arrow.down",disabled:!authoring || model.busy || model.selected.count != 1) { reorder(-1) }
            }.padding()
            ScrollView {
                LazyVStack(spacing:2) { ForEach(layers) { layer in layerRow(layer) } }
            }
            Spacer(minLength:0)
            Label("この Mac 内だけで制作",systemImage:"lock.shield").font(.caption).foregroundStyle(.secondary).padding()
        }.background(Color(NSColor.controlBackgroundColor))
    }
    private func layerRow(_ layer:EditorLayer) -> some View {
        let node=layer.node,id=layer.id,locked=EditorGeometry.locked(nodes,node:node)
        return HStack(spacing:6) {
            Button {
                let additive=NSEvent.modifierFlags.contains(.command) || NSEvent.modifierFlags.contains(.shift)
                Task { guard await model.commitFocusedEditing() else{return};select(id,additive:additive) }
            } label: {
                HStack(spacing:7) { Image(systemName:symbol(EditorJSON.string(node["type"])));Text(EditorJSON.string(node["name"])).lineLimit(1);Spacer() }.padding(.leading,CGFloat(8+layer.depth*12))
            }.buttonStyle(.plain)
            Button { Task { await model.edit(["op":"setVisible","nodeId":id,"visible":!EditorJSON.bool(node["visible"],true)]) } } label: { Image(systemName:EditorJSON.bool(node["visible"],true) ? "eye":"eye.slash") }.buttonStyle(.plain).disabled(!authoring || model.busy || locked).help("表示切替")
            Button { Task { await model.edit(["op":"setLocked","nodeId":id,"locked":!EditorJSON.bool(node["locked"])]) } } label: { Image(systemName:EditorJSON.bool(node["locked"]) ? "lock.fill":"lock.open") }.buttonStyle(.plain).disabled(!authoring || model.busy).help("ロック切替")
        }.font(.system(size:11)).padding(.vertical,8).padding(.trailing,8).background(model.selected.contains(id) ? Color.accentColor.opacity(0.14):Color.clear)
    }
    private var center:some View {
        VStack(spacing:0) {
            HStack(spacing:12) {
                iconButton("テキストを追加","textformat",disabled:!authoring || model.busy || (model.session?.document.fonts.isEmpty ?? true)) { Task { await model.addNode("text") } }
                iconButton("図形を追加","square",disabled:!authoring || model.busy) { Task { await model.addNode("rect") } }
                iconButton("ベジェパスを追加","point.topleft.down.to.point.bottomright.curvepath",disabled:!authoring || model.busy) { Task { await model.addNode("path") } }
                iconButton("画像を取り込む","photo.badge.plus",disabled:!authoring || model.busy) { Task { await model.importAsset("raster",addToCanvas:true) } }
                iconButton("ロゴを取り込む","seal",disabled:!authoring || model.busy) { Task { await model.importAsset("svg",addToCanvas:true) } }
                Divider().frame(height:18)
                iconButton("元に戻す","arrow.uturn.backward",disabled:model.busy || !(model.session?.canUndo ?? false)) { Task { await model.history("undo") } }
                iconButton("やり直す","arrow.uturn.forward",disabled:model.busy || !(model.session?.canRedo ?? false)) { Task { await model.history("redo") } }
                iconButton("複製","doc.on.doc",disabled:!authoring || model.busy || model.selected.isEmpty) { Task { await model.duplicateSelection() } }
                iconButton("削除","trash",disabled:!authoring || model.busy || model.selected.isEmpty) { Task { await model.deleteSelection() } }
                Spacer(minLength:4)
            }.padding(12)
            HStack(spacing:12) {
                Toggle("グリッド",isOn:$grid);Toggle("セーフエリア",isOn:$safe);Toggle("スナップ",isOn:$snap);Toggle("ガイド",isOn:$guides)
                if guides { TextField("縦 %",value:$guideX,format:.number).frame(width:50);TextField("横 %",value:$guideY,format:.number).frame(width:50) }
                Spacer()
            }.toggleStyle(.checkbox).font(.caption).padding(.horizontal,12).padding(.bottom,10)
            Divider()
            GeometryReader { proxy in
                let width=max(1,EditorJSON.number(variant["width"],1080)),height=max(1,EditorJSON.number(variant["height"],1080))
                let scale=max(0.02,min((proxy.size.width-64)/width,(proxy.size.height-72)/height))*zoom
                ScrollView([.horizontal,.vertical]) {
                    VStack(spacing:8) {
                        Text("\(EditorJSON.string(variant["label"])) · \(Int(width)) × \(Int(height)) px").font(.caption).foregroundStyle(.secondary)
                        EditorCanvas(nodes:nodes,styles:styles,rootID:rootID,selected:model.selected,preview:model.preview,loading:model.previewLoading,revision:model.session?.document.revision ?? 0,scale:scale,width:width,height:height,grid:grid,safe:safe,snap:snap,guides:guides,guideX:guideX,guideY:guideY,disabled:!authoring || model.busy,onSelect:select,onMove:{ changes in Task { await model.changeBounds(changes:changes) } },onDelete:{ Task { await model.deleteSelection() } },onDuplicate:{ Task { await model.duplicateSelection() } },onZoom:{delta in zoom=max(0.25,min(3,zoom+delta))})
                            .frame(width:width*scale,height:height*scale).shadow(color:.black.opacity(0.15),radius:12,y:4)
                        Text(model.previewLoading ? "正確プレビューを描画しています…":"Skia 共通描画 · sRGB").font(.caption2).foregroundStyle(.secondary)
                    }.padding(24).frame(minWidth:proxy.size.width,minHeight:proxy.size.height)
                }.background(Color(NSColor.underPageBackgroundColor))
            }
            Divider()
            HStack {
                Text(model.selected.isEmpty ? "レイヤーを選択して編集":"\(model.selected.count) レイヤーを選択").font(.caption).foregroundStyle(.secondary)
                Spacer();Button { zoom=max(0.25,zoom-0.1) } label: { Image(systemName:"minus.magnifyingglass") };Button("\(Int(zoom*100))%") { zoom=1 };Button { zoom=min(3,zoom+0.1) } label: { Image(systemName:"plus.magnifyingglass") }
            }.buttonStyle(.borderless).padding(10)
        }
    }
    private func select(_ id:String?,additive:Bool) {
        guard let id=id else { model.selected=[];return }
        if additive { if model.selected.contains(id) { model.selected.removeAll { $0==id } } else { model.selected.append(id) } }
        else { model.selected=[id] }
    }
    private func reorder(_ offset:Int) {
        guard let id=model.selected.first,let parentID=EditorGeometry.ancestorIDs(nodes,id:id).first,let parent=nodes.first(where:{EditorJSON.string($0["id"])==parentID}),let index=EditorJSON.strings(parent["childIds"]).firstIndex(of:id) else { return }
        let count=EditorJSON.strings(parent["childIds"]).count
        Task { await model.edit(["op":"reorder","nodeId":id,"index":max(0,min(count-1,index+offset))]) }
    }
    private func symbol(_ type:String) -> String { ["group":"folder","text":"textformat","image":"photo","logo":"seal","path":"point.topleft.down.to.point.bottomright.curvepath"][type] ?? "square" }
    private func iconButton(_ title:String,_ symbol:String,disabled:Bool,action:@escaping ()->Void) -> some View { Button(action:action) { Image(systemName:symbol).frame(width:16,height:16) }.buttonStyle(.borderless).disabled(disabled).help(title).accessibilityLabel(title) }
}

private struct EditorCanvas:NSViewRepresentable {
    var nodes:[[String:Any]];var styles:[[String:Any]];var rootID:String;var selected:[String];var preview:Data?;var loading:Bool;var revision:UInt64;var scale:Double;var width:Double;var height:Double
    var grid:Bool;var safe:Bool;var snap:Bool;var guides:Bool;var guideX:Double;var guideY:Double;var disabled:Bool
    var onSelect:(String?,Bool)->Void;var onMove:([[String:Any]])->Void;var onDelete:()->Void;var onDuplicate:()->Void;var onZoom:(Double)->Void
    func makeNSView(context:Context) -> NativeEditorCanvas { NativeEditorCanvas() }
    func updateNSView(_ view:NativeEditorCanvas,context:Context) {
        view.nodes=nodes;view.styles=styles;view.rootID=rootID;view.selected=selected;view.loading=loading;view.revision=revision;view.scale=scale;view.boardWidth=width;view.boardHeight=height
        view.grid=grid;view.safe=safe;view.snap=snap;view.guides=guides;view.guideX=guideX;view.guideY=guideY;view.disabled=disabled
        view.onSelect=onSelect;view.onMove=onMove;view.onDelete=onDelete;view.onDuplicate=onDuplicate;view.onZoom=onZoom
        if view.previewData != preview { view.previewData=preview;view.previewImage=preview.flatMap(NSImage.init(data:)) }
        view.needsDisplay=true
    }
}

private final class NativeEditorCanvas:NSView {
    var nodes:[[String:Any]]=[];var styles:[[String:Any]]=[];var rootID="";var selected:[String]=[];var previewData:Data?;var previewImage:NSImage?;var loading=false;var revision:UInt64=0;var scale=1.0;var boardWidth=1080.0;var boardHeight=1080.0
    var grid=false;var safe=false;var snap=true;var guides=false;var guideX=50.0;var guideY=50.0;var disabled=false
    var onSelect:(String?,Bool)->Void={_,_ in};var onMove:([[String:Any]])->Void={_ in};var onDelete:()->Void={};var onDuplicate:()->Void={};var onZoom:(Double)->Void={_ in}
    private struct Drag { var start:CGPoint;var nodes:[[String:Any]];var styles:[[String:Any]];var ids:[String];var resize:Bool;var revision:UInt64;var dx:Double=0;var dy:Double=0;var bounds:EditorBounds? }
    private var drag:Drag?
    override var isFlipped:Bool { true }
    override var acceptsFirstResponder:Bool { true }
    override func draw(_ dirtyRect:NSRect) {
        NSColor.white.setFill();bounds.fill()
        let check=NSBezierPath();let step=12.0
        for y in stride(from:0.0,to:bounds.height,by:step) { for x in stride(from:0.0,to:bounds.width,by:step) where (Int(x/step)+Int(y/step))%2==0 { check.appendRect(CGRect(x:x,y:y,width:step,height:step)) } }
        NSColor(calibratedWhite:0.94,alpha:1).setFill();check.fill()
        previewImage?.draw(in:bounds,from:.zero,operation:.sourceOver,fraction:1,respectFlipped:true,hints:[.interpolation:NSImageInterpolation.high])
        if previewImage == nil { let text=loading ? "正確プレビューを描画しています…":"検査項目を修正すると作品を表示します";(text as NSString).draw(at:CGPoint(x:20,y:20),withAttributes:[.font:NSFont.systemFont(ofSize:12),.foregroundColor:NSColor.secondaryLabelColor]) }
        if grid {
            let path=NSBezierPath();path.lineWidth=0.5
            for x in stride(from:0.0,to:bounds.width,by:max(2,24*scale)) { path.move(to:CGPoint(x:x,y:0));path.line(to:CGPoint(x:x,y:bounds.height)) }
            for y in stride(from:0.0,to:bounds.height,by:max(2,24*scale)) { path.move(to:CGPoint(x:0,y:y));path.line(to:CGPoint(x:bounds.width,y:y)) }
            NSColor.gray.withAlphaComponent(0.25).setStroke();path.stroke()
        }
        if safe { let path=NSBezierPath(rect:bounds.insetBy(dx:bounds.width*0.05,dy:bounds.height*0.05));path.setLineDash([4,4],count:2,phase:0);NSColor.systemOrange.withAlphaComponent(0.6).setStroke();path.stroke() }
        if guides {
            let path=NSBezierPath();let x=boardWidth*max(0,min(100,guideX.isFinite ? guideX:50))/100*scale,y=boardHeight*max(0,min(100,guideY.isFinite ? guideY:50))/100*scale
            path.move(to:CGPoint(x:x,y:0));path.line(to:CGPoint(x:x,y:bounds.height));path.move(to:CGPoint(x:0,y:y));path.line(to:CGPoint(x:bounds.width,y:y));NSColor.systemCyan.setStroke();path.stroke()
        }
        for node in EditorGeometry.targets(nodes,rootID:rootID,selected:selected) where selected.contains(EditorJSON.string(node["id"])) {
            let id=EditorJSON.string(node["id"]),position=EditorGeometry.frame(nodes,node:node,styles:styles)
            var box=EditorBounds(node["bounds"])
            if drag?.ids.contains(id)==true,drag?.resize==true,let updated=drag?.bounds { box=updated }
            let updatedNode: [String:Any] = { var copy=node;copy["bounds"]=box.raw;return copy }()
            let matrix=drag?.resize==true && drag?.ids.contains(id)==true ? EditorGeometry.frame(nodes,node:updatedNode,styles:styles):position
            let points=box.corners.map { point -> CGPoint in var p=matrix.point(point);if drag?.ids.contains(id)==true && drag?.resize==false { p.x+=drag?.dx ?? 0;p.y+=drag?.dy ?? 0 };return CGPoint(x:p.x*scale,y:p.y*scale) }
            let path=NSBezierPath();path.move(to:points[0]);for p in points.dropFirst() { path.line(to:p) };path.close();path.lineWidth=1.5;NSColor.controlAccentColor.setStroke();path.stroke()
            if !disabled && !EditorGeometry.locked(nodes,node:node) { for point in points { let handle=NSBezierPath(rect:CGRect(x:point.x-3,y:point.y-3,width:6,height:6));NSColor.white.setFill();handle.fill();NSColor.controlAccentColor.setStroke();handle.stroke() } }
        }
    }
    private func world(_ event:NSEvent) -> CGPoint { let p=convert(event.locationInWindow,from:nil);return CGPoint(x:p.x/scale,y:p.y/scale) }
    private func path(_ node:[String:Any]) -> NSBezierPath {
        let matrix=EditorGeometry.frame(nodes,node:node,styles:styles),points=EditorBounds(node["bounds"]).corners.map(matrix.point)
        let path=NSBezierPath();path.move(to:points[0]);for point in points.dropFirst() { path.line(to:point) };path.close();return path
    }
    override func mouseDown(with event:NSEvent) {
        window?.makeFirstResponder(self);let point=world(event)
        let targets=EditorGeometry.targets(nodes,rootID:rootID,selected:selected)
        if !disabled,let node=targets.reversed().first(where:{ node in
            let corner=EditorGeometry.frame(nodes,node:node,styles:styles).point(EditorBounds(node["bounds"]).corners[2])
            return selected.contains(EditorJSON.string(node["id"])) && !EditorGeometry.locked(nodes,node:node) && hypot(corner.x-point.x,corner.y-point.y)<=8/scale
        }) {
            drag=Drag(start:point,nodes:nodes,styles:styles,ids:[EditorJSON.string(node["id"])],resize:true,revision:revision);return
        }
        guard let node=targets.reversed().first(where:{path($0).contains(point)}) else { onSelect(nil,false);return }
        let id=EditorJSON.string(node["id"]),additive=event.modifierFlags.contains(.command)||event.modifierFlags.contains(.shift)
        let ids=(selected.contains(id) ? selected:[id]).filter { candidate in !EditorGeometry.ancestorIDs(nodes,id:candidate).contains { selected.contains($0) } && nodes.first { EditorJSON.string($0["id"])==candidate }.map { !EditorGeometry.locked(nodes,node:$0) } == true }
        if additive || !selected.contains(id) { onSelect(id,additive) }
        guard !disabled,!additive,!EditorGeometry.locked(nodes,node:node) else { return }
        drag=Drag(start:point,nodes:nodes,styles:styles,ids:ids,resize:false,revision:revision)
    }
    override func mouseDragged(with event:NSEvent) {
        guard var current=drag,current.revision==revision,!disabled else { drag=nil;return }
        let point=world(event),dx=point.x-current.start.x,dy=point.y-current.start.y,step=snap ? 8.0:1.0
        if current.resize,let node=current.nodes.first(where:{EditorJSON.string($0["id"])==current.ids.first}) {
            let delta=EditorGeometry.frame(current.nodes,node:node,styles:current.styles).inverseVector(dx:dx,dy:dy)
            let bounds=EditorBounds(node["bounds"]),dh=event.modifierFlags.contains(.shift) ? delta.dx*bounds.height/bounds.width:delta.dy
            current.bounds=EditorGeometry.resized(node,dw:(delta.dx/step).rounded()*step,dh:(dh/step).rounded()*step)
        } else { current.dx=(dx/step).rounded()*step;current.dy=(dy/step).rounded()*step }
        drag=current;needsDisplay=true
    }
    override func mouseUp(with event:NSEvent) {
        guard let current=drag,current.revision==revision,!disabled else { drag=nil;needsDisplay=true;return }
        drag=nil;needsDisplay=true
        if current.resize,let id=current.ids.first,let bounds=current.bounds { onMove([["id":id,"bounds":bounds.raw]]) }
        else if current.dx != 0 || current.dy != 0 {
            onMove(current.nodes.filter { current.ids.contains(EditorJSON.string($0["id"])) }.map { node in let delta=EditorGeometry.parentTransform(current.nodes,node:node,styles:current.styles).inverseVector(dx:current.dx,dy:current.dy);return ["id":EditorJSON.string(node["id"]),"bounds":EditorBounds(node["bounds"]).moved(delta.dx,delta.dy).raw] })
        }
    }
    override func keyDown(with event:NSEvent) {
        if event.keyCode==53 { drag=nil;onSelect(nil,false);needsDisplay=true;return }
        if event.characters=="+" || event.characters=="=" { onZoom(0.1);return }
        if event.characters=="-" { onZoom(-0.1);return }
        guard !disabled else { super.keyDown(with:event);return }
        if event.keyCode==51 || event.keyCode==117 { onDelete();return }
        if event.modifierFlags.contains(.command),event.charactersIgnoringModifiers?.lowercased()=="d" { onDuplicate();return }
        guard [123,124,125,126].contains(event.keyCode),!event.modifierFlags.contains(.command) else { super.keyDown(with:event);return }
        let step=event.modifierFlags.contains(.shift) ? 10.0:1.0,dx=event.keyCode==123 ? -step:event.keyCode==124 ? step:0,dy=event.keyCode==126 ? -step:event.keyCode==125 ? step:0
        let chosen=nodes.filter { node in let id=EditorJSON.string(node["id"]);return selected.contains(id) && !EditorGeometry.locked(nodes,node:node) && !EditorGeometry.ancestorIDs(nodes,id:id).contains { selected.contains($0) } }
        onMove(chosen.map { node in let delta=EditorGeometry.parentTransform(nodes,node:node,styles:styles).inverseVector(dx:dx,dy:dy);return ["id":EditorJSON.string(node["id"]),"bounds":EditorBounds(node["bounds"]).moved(delta.dx,delta.dy).raw] })
    }
}
