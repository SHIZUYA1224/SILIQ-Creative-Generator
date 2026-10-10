import SwiftUI
import AppKit

struct InspectorView:View {
    @ObservedObject var model:StudioModel
    private let document:Document?
    private let context:EditorCommitContext
    init(model:StudioModel) {
        self.model=model;document=model.session?.document
        context=EditorCommitContext(sessionID:model.session?.sessionId ?? "",variantID:model.variantID,nodeID:model.selected.first ?? "")
    }
    private var nodes:[[String:Any]] { EditorGeometry.resolvedNodes(document?.raw ?? [:],variantID:context.variantID) }
    private var node:[String:Any]? { nodes.first { EditorJSON.string($0["id"])==context.nodeID } }
    private var type:String { EditorJSON.string(node?["type"]) }
    private var nodeID:String { context.nodeID }
    private var blocked:Bool { model.busy || !model.authoring || node.map { EditorGeometry.locked(nodes,node:$0) } == true }
    private var valueBlocked:Bool { model.busy || node.map { EditorGeometry.locked(nodes,node:$0) } == true || (!model.authoring && slot == nil) }
    private var variant:[String:Any] { document?.variant(context.variantID) ?? [:] }
    private var style:[String:Any] { document?.nodeStyles.first { EditorJSON.string($0["nodeId"])==nodeID } ?? ["nodeId":nodeID] }
    private var text:[String:Any] { EditorJSON.object(node?["text"]) }
    private var slot:[String:Any]? { document?.slots.first { EditorJSON.string($0["nodeId"])==nodeID } }

    var body:some View {
        VStack(alignment:.leading,spacing:0) {
            HStack { Text(node.map { EditorJSON.string($0["name"]) } ?? "プロジェクト設定").font(.headline).lineLimit(1);Spacer();if !type.isEmpty { Text(type.uppercased()).font(.caption2).foregroundStyle(.secondary) } }.padding()
            Divider()
            ScrollView {
                VStack(alignment:.leading,spacing:18) {
                    if document != nil {
                        if node != nil {
                            geometrySection
                            if type=="text" { textSection;textEffectsSection }
                            if type=="rect" || type=="path" { paintSection }
                            if type=="path" { NativePathEditor(node:node!,disabled:blocked,onCommand:send) }
                            if type=="image" || type=="logo" { mediaSection }
                            effectsSection
                            if type=="text" || type=="image" { slotSection }
                        } else { projectSection;fontSection }
                        NativeBrandEditor(model:model,node:node,disabled:blocked,document:document,context:context)
                        approvalSection
                    } else { Text("作品を開いて編集を始めてください。").foregroundStyle(.secondary) }
                }.padding(16).id("\(context.sessionID)-\(nodeID)-\(context.variantID)")
            }
        }.background(Color(NSColor.controlBackgroundColor))
    }
    private func send(_ command:[String:Any]) { Task { await model.edit(command,context:context) } }
    private func command(_ op:String,_ fields:[String:Any]) { send(context.nodeCommand(op,fields:fields)) }
    private func textStyle(_ fields:[String:Any]) { command("setTextStyle",["style":fields]) }
    private func nodeStyle(_ fields:[String:Any]) { var next=style;for (key,value) in fields { next[key]=value };command("setNodeStyle",["style":next]) }
    private func variantChange(_ fields:[String:Any]) { var next=variant;for (key,value) in fields { next[key]=value };next["approvalStatus"]="draft";send(["op":"setVariant","variant":next]) }
    private var geometrySection:some View {
        let bounds=EditorBounds(node?["bounds"])
        return InspectorSection("配置とサイズ") {
            Grid(horizontalSpacing:10,verticalSpacing:8) {
                GridRow { number("X",bounds.x,min:-1_000_000,max:1_000_000) { var next=bounds;next.x=$0;changeBounds(next) };number("Y",bounds.y,min:-1_000_000,max:1_000_000) { var next=bounds;next.y=$0;changeBounds(next) } }
                GridRow { number("幅",bounds.width,min:1,max:16384) { var next=bounds;next.width=$0;changeBounds(next) };number("高さ",bounds.height,min:1,max:16384) { var next=bounds;next.height=$0;changeBounds(next) } }
                GridRow { number("回転 °",EditorJSON.number(node?["rotationDeg"]),min:-360,max:360) { command("setRotation",["rotationDeg":$0]) };number("不透明度 %",EditorJSON.number(node?["opacity"],1)*100,min:0,max:100) { command("setOpacity",["opacity":$0/100]) } }
            }
            if model.selected.count>1 {
                HStack { ForEach(["left","center","right","top","middle","bottom"],id:\.self) { direction in Button(["left":"左","center":"中央","right":"右","top":"上","middle":"中間","bottom":"下"][direction]!) { align(direction) }.disabled(blocked) } }.font(.caption)
            }
        }
    }
    private func changeBounds(_ value:EditorBounds) {
        guard let document else{return}
        send(EditorGeometry.boundsCommand(document.raw,variantID:context.variantID,changes:[["id":nodeID,"bounds":value.raw]]))
    }
    private func align(_ direction:String) {
        let chosen=nodes.filter { node in let id=EditorJSON.string(node["id"]);return model.selected.contains(id) && !EditorGeometry.ancestorIDs(nodes,id:id).contains { model.selected.contains($0) } }
        let styles=EditorGeometry.resolvedStyles(document?.raw ?? [:],variantID:context.variantID)
        let boxes=chosen.map { node -> EditorBounds in let points=EditorBounds(node["bounds"]).corners.map(EditorGeometry.frame(nodes,node:node,styles:styles).point);let xs=points.map(\.x),ys=points.map(\.y);return EditorBounds(x:xs.min()!,y:ys.min()!,width:xs.max()!-xs.min()!,height:ys.max()!-ys.min()!) }
        guard let left=boxes.map(\.x).min(),let top=boxes.map(\.y).min(),let right=boxes.map({$0.x+$0.width}).max(),let bottom=boxes.map({$0.y+$0.height}).max() else { return }
        let changes=chosen.enumerated().map { index,node -> [String:Any] in let b=boxes[index];let x=direction=="left" ? left:direction=="center" ? (left+right-b.width)/2:direction=="right" ? right-b.width:b.x;let y=direction=="top" ? top:direction=="middle" ? (top+bottom-b.height)/2:direction=="bottom" ? bottom-b.height:b.y;let delta=EditorGeometry.parentTransform(nodes,node:node,styles:styles).inverseVector(dx:x-b.x,dy:y-b.y);return ["id":EditorJSON.string(node["id"]),"bounds":EditorBounds(node["bounds"]).moved(delta.dx,delta.dy).raw] }
        if let document { send(EditorGeometry.boundsCommand(document.raw,variantID:context.variantID,changes:changes)) }
    }
    private var textSection:some View {
        InspectorSection("テキスト") {
            NativeCommitText("本文",value:EditorJSON.string(text["value"]),multiline:true,disabled:valueBlocked) { command("setText",["value":$0]) }
            choice("フォント",EditorJSON.string(text["fontId"]),document?.fonts.map { (EditorJSON.string($0["id"]),EditorJSON.string($0["nameHint"])) } ?? []) { textStyle(["fontId":$0]) }
            Grid(horizontalSpacing:10,verticalSpacing:8) {
                GridRow { number("文字サイズ",EditorJSON.number(text["fontSize"]),min:1,max:2000) { size in var patch:[String:Any]=["fontSize":size];if EditorJSON.string(text["overflowPolicy"])=="fit-approved-sizes" { patch["allowedFontSizes"]=[size]+(text["allowedFontSizes"] as? [Double] ?? []).filter { $0<size } };textStyle(patch) };number("行間",EditorJSON.number(text["lineHeight"]),min:1,max:4000) { textStyle(["lineHeight":$0]) } }
                GridRow { number("字間",EditorJSON.number(text["letterSpacing"]),min:-100,max:100) { textStyle(["letterSpacing":$0]) };number("最大行数",EditorJSON.number(text["maxLines"]),min:1,max:100,integer:true) { textStyle(["maxLines":Int($0)]) } }
            }
            choice("段落の揃え",EditorJSON.string(text["align"]),[("left","左揃え"),("center","中央揃え"),("right","右揃え")]) { textStyle(["align":$0]) }
            NativeColorField("文字色",hex:EditorJSON.string(EditorJSON.object(node?["fill"])["color"],"#202723"),disabled:blocked) { textStyle(["color":$0]) }
            choice("あふれ時の処理",EditorJSON.string(text["overflowPolicy"]),[("error","エラーで停止"),("fit-approved-sizes","許可サイズから調整")]) { value in textStyle(["overflowPolicy":value,"allowedFontSizes":value=="error" ? []:[EditorJSON.number(text["fontSize"])]]) }
            if EditorJSON.string(text["overflowPolicy"])=="fit-approved-sizes" { NativeCommitText("許可サイズ（大きい順、カンマ区切り）",value:(text["allowedFontSizes"] as? [NSNumber] ?? []).map { $0.stringValue }.joined(separator:", "),disabled:blocked) { value in let values=value.split(separator:",",omittingEmptySubsequences:false).compactMap { Double($0.trimmingCharacters(in:.whitespaces)) };if values.count != value.split(separator:",",omittingEmptySubsequences:false).count { model.reportError(EditorPath.Failure.syntax("許可サイズに数値を指定してください。")) } else { textStyle(["allowedFontSizes":values]) } } }
            choice("言語",EditorJSON.string(text["language"],"ja"),[("ja","日本語"),("en","英語")]) { textStyle(["language":$0,"kinsoku":$0=="en" ? "unicode":EditorJSON.string(text["kinsoku"],"unicode")]) }
            choice("禁則処理",EditorJSON.string(text["kinsoku"],"unicode"),EditorJSON.string(text["language"],"ja")=="ja" ? [("unicode","標準の改行規則"),("strict-japanese","日本語の厳密な禁則")]:[("unicode","標準の改行規則")]) { textStyle(["kinsoku":$0]) }
            NativeTextAdvanced(text:text,fonts:document?.fonts ?? [],disabled:blocked,onStyle:textStyle)
        }
    }
    private var textEffectsSection:some View {
        let outline=EditorJSON.object(node?["outline"]),shadow=EditorJSON.object(node?["shadow"])
        return InspectorSection("文字のフチと影") {
            Toggle("フチを付ける",isOn:Binding(get:{!outline.isEmpty},set:{ value in command("setTextEffects",["outline":value ? ["color":"#ffffff","width":2]:NSNull(),"shadow":shadow.isEmpty ? NSNull():shadow]) })).disabled(blocked)
            if !outline.isEmpty { number("フチ幅",EditorJSON.number(outline["width"]),min:0,max:64) { value in var next=outline;next["width"]=value;command("setTextEffects",["outline":next,"shadow":shadow.isEmpty ? NSNull():shadow]) };NativeColorField("フチ色",hex:EditorJSON.string(outline["color"]),disabled:blocked) { value in var next=outline;next["color"]=value;command("setTextEffects",["outline":next,"shadow":shadow.isEmpty ? NSNull():shadow]) } }
            Toggle("文字に影を付ける",isOn:Binding(get:{!shadow.isEmpty},set:{ value in command("setTextEffects",["outline":outline.isEmpty ? NSNull():outline,"shadow":value ? ["color":"#000000","opacity":0.25,"offsetX":2,"offsetY":4,"blur":3]:NSNull()]) })).disabled(blocked)
            if !shadow.isEmpty {
                ForEach(["offsetX","offsetY","blur"],id:\.self) { key in number(["offsetX":"影 X","offsetY":"影 Y","blur":"影のぼかし"][key]!,EditorJSON.number(shadow[key]),min:key=="blur" ? 0:-256,max:key=="blur" ? 128:256) { value in var next=shadow;next[key]=value;command("setTextEffects",["outline":outline.isEmpty ? NSNull():outline,"shadow":next]) } }
                NativeColorField("影の色",hex:EditorJSON.string(shadow["color"]),disabled:blocked) { value in var next=shadow;next["color"]=value;command("setTextEffects",["outline":outline.isEmpty ? NSNull():outline,"shadow":next]) }
            }
        }
    }
    private var paintSection:some View {
        let fill=EditorJSON.object(node?["fill"]),stroke=EditorJSON.object(style["stroke"])
        return InspectorSection("塗りと線") {
            NativePaintEditor(paint:fill,disabled:blocked) { command("setFill",["fill":$0]) }
            if type=="rect" { number("角の丸み",EditorJSON.number(node?["cornerRadius"]),min:0,max:8192) { command("setRectCornerRadius",["cornerRadius":$0]) } }
            Toggle("線を付ける",isOn:Binding(get:{!stroke.isEmpty},set:{nodeStyle(["stroke":$0 ? ["color":"#202723","width":2]:NSNull()])})).disabled(blocked)
            if !stroke.isEmpty { number("線幅",EditorJSON.number(stroke["width"]),min:0.1,max:128) { value in var next=stroke;next["width"]=value;nodeStyle(["stroke":next]) };NativeColorField("線の色",hex:EditorJSON.string(stroke["color"]),disabled:blocked) { value in var next=stroke;next["color"]=value;nodeStyle(["stroke":next]) } }
        }
    }
    private var mediaSection:some View {
        let brand=document?.brandSnapshot,permitted=EditorJSON.strings(brand?["logoAssetIds"])
        let assets=document?.assets.filter { type=="image" ? EditorJSON.string($0["kind"])=="raster":brand == nil || permitted.contains(EditorJSON.string($0["id"])) } ?? []
        let adjustments=EditorJSON.object(style["imageAdjustments"])
        return InspectorSection(type=="logo" ? "ロゴ":"画像") {
            NativeChoice("素材",value:EditorJSON.string(node?["assetId"]),options:assets.map { (EditorJSON.string($0["id"]),EditorJSON.string($0["id"])) },disabled:valueBlocked || (type=="logo" && !model.authoring)) { command("setImageAsset",["assetId":$0]) }
            Button("素材を取り込む") { Task { await model.importAsset(type=="logo" ? "svg":"raster",addToCanvas:false) } }.disabled(blocked)
            if type=="image" {
                choice("画像の収め方",EditorJSON.string(node?["fit"]),[("cover","枠を埋める"),("contain","全体を収める"),("stretch","縦横比を変えて合わせる")]) { fit($0) }
                let focal=EditorJSON.object(node?["focalPoint"])
                HStack { number("焦点 X %",EditorJSON.number(focal["x"],0.5)*100,min:0,max:100) { value in var next=focal;next["x"]=value/100;fit(focal:next) };number("焦点 Y %",EditorJSON.number(focal["y"],0.5)*100,min:0,max:100) { value in var next=focal;next["y"]=value/100;fit(focal:next) } }
                Toggle("枠の外を切り抜く",isOn:Binding(get:{EditorJSON.bool(node?["clipToBounds"],true)},set:{fit(clip:$0)})).disabled(blocked)
                forAdjustment("明るさ","brightness",adjustments,-1,1,0)
                forAdjustment("コントラスト","contrast",adjustments,0,3,1)
                forAdjustment("彩度","saturation",adjustments,0,3,1)
            } else { Text("ロゴの縦横比と指定された使用条件を保持します。").font(.caption).foregroundStyle(.secondary) }
        }
    }
    private func fit(_ value:String?=nil,focal:[String:Any]?=nil,clip:Bool?=nil) { command("setImageFit",["fit":value ?? EditorJSON.string(node?["fit"]),"focalPoint":focal ?? EditorJSON.object(node?["focalPoint"]),"clipToBounds":clip ?? EditorJSON.bool(node?["clipToBounds"],true)]) }
    private func forAdjustment(_ title:String,_ key:String,_ adjustments:[String:Any],_ min:Double,_ max:Double,_ fallback:Double) -> some View { number(title,EditorJSON.number(adjustments[key],fallback),min:min,max:max) { value in var next:[String:Any]=["brightness":EditorJSON.number(adjustments["brightness"]),"contrast":EditorJSON.number(adjustments["contrast"],1),"saturation":EditorJSON.number(adjustments["saturation"],1)];next[key]=value;nodeStyle(["imageAdjustments":next]) } }
    private var effectsSection:some View {
        let effects=EditorJSON.objects(style["effects"]),mask=EditorJSON.object(style["mask"]),clip=EditorJSON.object(style["clip"])
        return InspectorSection("合成と効果") {
            choice("描画モード",EditorJSON.string(style["blendMode"],"normal"),[("normal","通常"),("multiply","乗算"),("screen","スクリーン")]) { nodeStyle(["blendMode":$0]) }
            Text("保存された効果を上から順に適用します。").font(.caption).foregroundStyle(.secondary)
            ForEach(effects.indices,id:\.self) { index in
                let effect=effects[index],kind=EditorJSON.string(effect["kind"])
                VStack(alignment:.leading,spacing:6) {
                    HStack { Text(kind=="blur" ? "ぼかし":"影");Spacer();Button("↑") { var next=effects;next.swapAt(index,index-1);nodeStyle(["effects":next]) }.disabled(blocked || index==0);Button("↓") { var next=effects;next.swapAt(index,index+1);nodeStyle(["effects":next]) }.disabled(blocked || index+1==effects.count);Button("削除") { var next=effects;next.remove(at:index);nodeStyle(["effects":next]) }.disabled(blocked) }
                    if kind=="blur" { number("半径",EditorJSON.number(effect["radius"]),min:0.001,max:200) { value in var next=effects;next[index]["radius"]=value;nodeStyle(["effects":next]) } }
                    else {
                        ForEach(["offsetX","offsetY","blur","opacity"],id:\.self) { key in number(["offsetX":"影 X","offsetY":"影 Y","blur":"ぼかし","opacity":"不透明度"][key]!,EditorJSON.number(effect[key]),min:key=="offsetX" || key=="offsetY" ? -256:0,max:key=="opacity" ? 1:key=="blur" ? 128:256) { value in var next=effects;next[index][key]=value;nodeStyle(["effects":next]) } }
                        NativeColorField("影の色",hex:EditorJSON.string(effect["color"]),disabled:blocked) { value in var next=effects;next[index]["color"]=value;nodeStyle(["effects":next]) }
                    }
                }.padding(8).background(Color.primary.opacity(0.04)).cornerRadius(6)
            }
            HStack { Button("ぼかしを追加") { nodeStyle(["effects":effects+[["kind":"blur","radius":4]]]) };Button("影を追加") { nodeStyle(["effects":effects+[["kind":"shadow","color":"#000000","opacity":0.25,"offsetX":6,"offsetY":8,"blur":8]]]) } }.disabled(blocked || effects.count>=8)
            NativeCommitText("クリップパス（空欄で解除）",value:EditorJSON.string(clip["data"]),multiline:true,disabled:blocked) { data in nodeStyle(["clip":data.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty ? NSNull():["data":data,"fillRule":EditorJSON.string(clip["fillRule"],"non-zero")]]) }
            NativeCommitText("マスクパス（空欄で解除）",value:EditorJSON.string(mask["data"]),multiline:true,disabled:blocked) { data in nodeStyle(["mask":data.trimmingCharacters(in:.whitespacesAndNewlines).isEmpty ? NSNull():["data":data,"fillRule":EditorJSON.string(mask["fillRule"],"non-zero"),"opacity":EditorJSON.number(mask["opacity"],1),"invert":EditorJSON.bool(mask["invert"])]]) }
            if !mask.isEmpty { number("マスクの不透明度 %",EditorJSON.number(mask["opacity"],1)*100,min:0,max:100) { value in var next=mask;next["opacity"]=value/100;nodeStyle(["mask":next]) };Toggle("マスクを反転",isOn:Binding(get:{EditorJSON.bool(mask["invert"])},set:{value in var next=mask;next["invert"]=value;nodeStyle(["mask":next])})).disabled(blocked) }
        }
    }
    private var slotSection:some View {
        InspectorSection("量産用スロット") {
            Toggle("差し替えを許可する",isOn:Binding(get:{slot != nil},set:{ value in
                if let slot=slot { send(["op":"removeSlot","slotId":EditorJSON.string(slot["id"])]) }
                else if value { var next:[String:Any]=["id":String(("slot-"+nodeID).prefix(64)),"type":type=="text" ? "text":"image","required":true,"nodeId":nodeID,"target":type=="text" ? "text.value":"assetId"];if type=="text" { next["maxLength"]=200 };send(["op":"setSlot","slot":next]) }
            })).disabled(blocked)
            if let slot=slot { Text("CSV / JSON の項目名: \(EditorJSON.string(slot["id"]))").font(.caption).textSelection(.enabled);Toggle("必須項目",isOn:Binding(get:{EditorJSON.bool(slot["required"])},set:{value in var next=slot;next["required"]=value;send(["op":"setSlot","slot":next])})).disabled(blocked);if type=="text" { number("最大文字数",EditorJSON.number(slot["maxLength"],200),min:1,max:10000,integer:true) { value in var next=slot;next["maxLength"]=Int(value);send(["op":"setSlot","slot":next]) } } }
        }
    }
    private var projectSection:some View {
        InspectorSection("プロジェクトとアートボード") {
            NativeCommitText("作品名",value:document?.title ?? "",disabled:blocked) { send(["op":"setTitle","title":$0]) }
            NativeCommitText("アートボード名",value:EditorJSON.string(variant["label"]),disabled:blocked) { variantChange(["label":$0]) }
            HStack { number("出力幅 px",EditorJSON.number(variant["width"]),min:1,max:16384,integer:true) { variantChange(["width":$0]) };number("出力高さ px",EditorJSON.number(variant["height"]),min:1,max:16384,integer:true) { variantChange(["height":$0]) } }
            Button("アートボードを複製") { variantChange(["id":"artboard-"+UUID().uuidString.lowercased().prefix(8),"label":EditorJSON.string(variant["label"])+" コピー"]) }.disabled(blocked)
            if (document?.variants.count ?? 0)>1 { Button("このアートボードを削除",role:.destructive) { send(["op":"removeVariant","variantId":context.variantID]) }.disabled(blocked) }
            Text("画像・文字・素材を .siliqcg にまとめて保存します。").font(.caption).foregroundStyle(.secondary)
        }
    }
    private var fontSection:some View {
        InspectorSection("使用フォント") {
            ForEach((document?.fonts ?? []).indices,id:\.self) { index in
                let font=document!.fonts[index]
                VStack(alignment:.leading,spacing:7) { Text(EditorJSON.string(font["nameHint"])).font(.subheadline);Text(EditorJSON.string(font["state"])=="resolved" ? "ローカルファイルを登録済み":"登録が必要です").font(.caption).foregroundStyle(.secondary);Button("同じフォントを再登録") { Task { await model.resolveFont(EditorJSON.string(font["id"])) } }.disabled(blocked || !(font["sha256"] is String));Toggle("持ち出し権利を確認して同梱",isOn:Binding(get:{EditorJSON.string(font["packagePolicy"])=="embed-confirmed"},set:{value in send(["op":"setFontPolicy","fontId":EditorJSON.string(font["id"]),"packagePolicy":value ? "embed-confirmed":"reference-only"])})).disabled(blocked) }.padding(8).background(Color.primary.opacity(0.04)).cornerRadius(6)
            }
            Button("ローカルフォントを登録") { Task { await model.importAsset("font",addToCanvas:false) } }.disabled(blocked)
        }
    }
    private var approvalSection:some View {
        InspectorSection("レイアウトの承認") {
            Label(EditorJSON.string(variant["approvalStatus"])=="approved" ? "このサイズは承認済み":"編集後の作品を確認してください",systemImage:EditorJSON.string(variant["approvalStatus"])=="approved" ? "checkmark.seal":"exclamationmark.circle").foregroundStyle(EditorJSON.string(variant["approvalStatus"])=="approved" ? Color.green:Color.orange)
            Button("このアートボードを承認する") { send(["op":"setVariantApproval","variantId":context.variantID,"approvalStatus":"approved"]) }.disabled(model.busy || !model.authoring || EditorJSON.string(variant["approvalStatus"])=="approved")
            Text("編集すると承認が解除されます。量産には承認済みのサイズを使います。").font(.caption).foregroundStyle(.secondary)
        }
    }
    private func number(_ label:String,_ value:Double,min:Double,max:Double,integer:Bool=false,onCommit:@escaping (Double)->Void) -> some View { NativeNumberField(label,value:value,min:min,max:max,integer:integer,disabled:blocked,onCommit:onCommit) }
    private func choice(_ label:String,_ value:String,_ options:[(String,String)],onChange:@escaping (String)->Void) -> some View { NativeChoice(label,value:value,options:options,disabled:blocked,onChange:onChange) }
}

struct InspectorSection<Content:View>:View {
    var title:String;@ViewBuilder var content:()->Content
    init(_ title:String,@ViewBuilder content:@escaping ()->Content) { self.title=title;self.content=content }
    var body:some View { VStack(alignment:.leading,spacing:9) { Text(title).font(.headline);content() }.frame(maxWidth:.infinity,alignment:.leading) }
}
struct NativeChoice:View {
    var label:String;var value:String;var options:[(String,String)];var disabled:Bool;var onChange:(String)->Void
    init(_ label:String,value:String,options:[(String,String)],disabled:Bool,onChange:@escaping (String)->Void) { self.label=label;self.value=value;self.options=options;self.disabled=disabled;self.onChange=onChange }
    var body:some View { Picker(label,selection:Binding(get:{value},set:onChange)) { ForEach(options.indices,id:\.self) { index in Text(options[index].1).tag(options[index].0) } }.disabled(disabled) }
}
struct NativeNumberField:View {
    var label:String;var value:Double;var min:Double;var max:Double;var integer:Bool;var disabled:Bool;var onCommit:(Double)->Void
    @State private var draft="";@State private var sent="";@State private var invalid=false;@FocusState private var focused:Bool
    init(_ label:String,value:Double,min:Double,max:Double,integer:Bool=false,disabled:Bool=false,onCommit:@escaping (Double)->Void) { self.label=label;self.value=value;self.min=min;self.max=max;self.integer=integer;self.disabled=disabled;self.onCommit=onCommit }
    var body:some View {
        VStack(alignment:.leading,spacing:3) { Text(label).font(.caption).foregroundStyle(.secondary);TextField(label,text:$draft).labelsHidden().textFieldStyle(.roundedBorder).focused($focused).disabled(disabled).onSubmit(commit);if invalid { Text("\(format(min))〜\(format(max))\(integer ? "の整数":"の数値")").font(.caption2).foregroundStyle(.red) } }
            .onAppear { draft=format(value);sent=draft }.onChange(of:value) { next in if !focused { draft=format(next);sent=draft;invalid=false } }.onChange(of:focused) { focus in if !focus { commit() } }
    }
    private func format(_ value:Double) -> String { value.rounded()==value ? String(Int(value)):String(value) }
    private func commit() { guard !disabled,draft != sent else { return };guard let number=Double(draft),number.isFinite,number>=min,number<=max,!integer || number.rounded()==number else { invalid=true;return };invalid=false;sent=draft;onCommit(number) }
}
struct NativeColorField:View {
    var label:String;var hex:String;var disabled:Bool;var onChange:(String)->Void
    init(_ label:String,hex:String,disabled:Bool,onChange:@escaping (String)->Void) { self.label=label;self.hex=hex;self.disabled=disabled;self.onChange=onChange }
    var body:some View { ColorPicker(label,selection:Binding(get:{ Self.color(hex) },set:{color in guard let rgb=NSColor(color).usingColorSpace(.sRGB) else { return };func channel(_ value:CGFloat)->Int { Int((max(0,min(1,value))*255).rounded()) };onChange(String(format:"#%02x%02x%02x",channel(rgb.redComponent),channel(rgb.greenComponent),channel(rgb.blueComponent))) }),supportsOpacity:false).disabled(disabled) }
    static func color(_ hex:String) -> Color { let value=UInt32(hex.trimmingCharacters(in:CharacterSet(charactersIn:"#")),radix:16) ?? 0;return Color(.sRGB,red:Double((value>>16)&255)/255,green:Double((value>>8)&255)/255,blue:Double(value&255)/255,opacity:1) }
}

struct NativeCommitText:View {
    var label:String;var value:String;var multiline:Bool;var disabled:Bool;var onCommit:(String)->Void
    @State private var commitSignal=0;@State private var marked=false
    init(_ label:String,value:String,multiline:Bool=false,disabled:Bool=false,onCommit:@escaping (String)->Void) { self.label=label;self.value=value;self.multiline=multiline;self.disabled=disabled;self.onCommit=onCommit }
    var body:some View { VStack(alignment:.leading,spacing:5) { Text(label).font(.caption).foregroundStyle(.secondary);IMETextRepresentable(value:value,multiline:multiline,disabled:disabled,commitSignal:commitSignal,onCommit:onCommit,onMarked:{marked=$0}).frame(height:multiline ? 100:38).background(Color(NSColor.textBackgroundColor)).cornerRadius(5).overlay(RoundedRectangle(cornerRadius:5).stroke(Color.primary.opacity(0.15)));HStack { if marked { Text("日本語変換の確定を待っています").font(.caption2).foregroundStyle(.secondary) };Spacer();Button("反映") { commitSignal+=1 }.font(.caption).disabled(disabled || marked) } } }
}
private struct IMETextRepresentable:NSViewRepresentable {
    var value:String;var multiline:Bool;var disabled:Bool;var commitSignal:Int;var onCommit:(String)->Void;var onMarked:(Bool)->Void
    func makeNSView(context:Context) -> NSScrollView {
        let scroll=NSScrollView(),text=ConfirmedTextView(frame:NSRect(x:0,y:0,width:280,height:multiline ? 100:38))
        scroll.hasVerticalScroller=multiline;scroll.drawsBackground=false;scroll.borderType = .noBorder;scroll.documentView=text
        text.isRichText=false;text.importsGraphics=false;text.allowsUndo=true;text.font = .systemFont(ofSize:13);text.isVerticallyResizable=true;text.isHorizontallyResizable=false;text.autoresizingMask=[.width];text.textContainer?.widthTracksTextView=true;text.textContainer?.containerSize=NSSize(width:0,height:CGFloat.greatestFiniteMagnitude);text.textContainerInset=NSSize(width:5,height:5)
        text.minSize=NSSize(width:0,height:multiline ? 100:38);text.maxSize=NSSize(width:CGFloat.greatestFiniteMagnitude,height:CGFloat.greatestFiniteMagnitude)
        text.string=value;text.source=value;text.committed=value;text.multiline=multiline;text.onCommit=onCommit;text.onMarked=onMarked;text.lastSignal=commitSignal
        return scroll
    }
    func updateNSView(_ scroll:NSScrollView,context:Context) {
        guard let text=scroll.documentView as? ConfirmedTextView else { return }
        text.onCommit=onCommit;text.onMarked=onMarked;text.isEditable = !disabled;text.isSelectable=true;text.multiline=multiline
        if !text.source.utf8.elementsEqual(value.utf8) && text.window?.firstResponder !== text && !text.hasMarkedText() { text.string=value;text.source=value;text.committed=value }
        if text.lastSignal != commitSignal { text.lastSignal=commitSignal;text.commitConfirmed() }
    }
    static func dismantleNSView(_ scroll:NSScrollView,coordinator:()) { if let text=scroll.documentView as? ConfirmedTextView { text.commitConfirmed();text.onCommit={_ in};text.onMarked={_ in} } }
}
final class ConfirmedTextView:NSTextView {
    var source="";var committed="";var lastSignal=0;var multiline=false;var pendingBlur=false
    var onCommit:(String)->Void={_ in};var onMarked:(Bool)->Void={_ in}
    override func didChangeText() { super.didChangeText();onMarked(hasMarkedText());if pendingBlur && !hasMarkedText() { pendingBlur=false;commitConfirmed() } }
    override func unmarkText() { super.unmarkText();onMarked(false);if pendingBlur { pendingBlur=false;commitConfirmed() } }
    override func resignFirstResponder() -> Bool { let allowed=super.resignFirstResponder();if allowed { if hasMarkedText() { pendingBlur=true } else { commitConfirmed() } };return allowed }
    override func insertNewline(_ sender:Any?) { if multiline || hasMarkedText() { super.insertNewline(sender) } else { commitConfirmed();window?.makeFirstResponder(nil) } }
    func commitConfirmed() { guard isEditable,!hasMarkedText(),!string.utf8.elementsEqual(committed.utf8) else { return };committed=string;source=string;onCommit(string) }
}

private struct NativePaintEditor:View {
    var paint:[String:Any];var disabled:Bool;var onChange:([String:Any])->Void
    var body:some View {
        let kind=EditorJSON.string(paint["kind"],"solid"),stops=EditorJSON.objects(paint["stops"])
        VStack(alignment:.leading,spacing:8) {
            NativeChoice("塗りの種類",value:kind,options:[("solid","単色"),("linear-gradient","線形グラデーション"),("radial-gradient","放射グラデーション")],disabled:disabled) { next in
                if next=="solid" { onChange(["kind":"solid","color":"#39735b"]) }
                else { var value:[String:Any]=["kind":next,"stops":stops.isEmpty ? [["offset":0,"color":"#39735b","opacity":1],["offset":1,"color":"#dbe5ce","opacity":1]]:stops];if next=="linear-gradient" { value["angleDeg"]=90 } else { value["center"]=["x":0.5,"y":0.5];value["radius"]=0.7 };onChange(value) }
            }
            if kind=="solid" { NativeColorField("塗りの色",hex:EditorJSON.string(paint["color"]),disabled:disabled) { var next=paint;next["color"]=$0;onChange(next) } }
            else {
                ForEach(stops.indices,id:\.self) { index in
                    NativeColorField("色 \(index+1)",hex:EditorJSON.string(stops[index]["color"]),disabled:disabled) { value in var next=paint,colors=stops;colors[index]["color"]=value;next["stops"]=colors;onChange(next) }
                    HStack { NativeNumberField("位置 %",value:EditorJSON.number(stops[index]["offset"])*100,min:0,max:100,disabled:disabled) { value in var next=paint,colors=stops;colors[index]["offset"]=value/100;next["stops"]=colors;onChange(next) };NativeNumberField("不透明度 %",value:EditorJSON.number(stops[index]["opacity"],1)*100,min:0,max:100,disabled:disabled) { value in var next=paint,colors=stops;colors[index]["opacity"]=value/100;next["stops"]=colors;onChange(next) } }
                }
                if kind=="linear-gradient" { NativeNumberField("角度 °",value:EditorJSON.number(paint["angleDeg"]),min:-360,max:360,disabled:disabled) { var next=paint;next["angleDeg"]=$0;onChange(next) } }
                else {
                    NativeNumberField("半径",value:EditorJSON.number(paint["radius"],0.7),min:0.001,max:2,disabled:disabled) { var next=paint;next["radius"]=$0;onChange(next) }
                    let center=EditorJSON.object(paint["center"])
                    ForEach(["x","y"],id:\.self) { axis in NativeNumberField("中心 \(axis.uppercased()) %",value:EditorJSON.number(center[axis],0.5)*100,min:0,max:100,disabled:disabled) { value in var next=paint,c=center;c[axis]=value/100;next["center"]=c;onChange(next) } }
                }
            }
        }
    }
}

private struct NativeTextAdvanced:View {
    var text:[String:Any];var fonts:[[String:Any]];var disabled:Bool;var onStyle:([String:Any])->Void
    @State private var start=0.0;@State private var end=1.0;@State private var font="";@State private var color="#202723"
    @State private var featureError=""
    private var characters:[Character] { Array(EditorJSON.string(text["value"])) }
    private var runs:[[String:Any]] { EditorJSON.objects(text["runs"]) }
    var body:some View {
        VStack(alignment:.leading,spacing:8) {
            NativeCommitText("OpenType 機能（kern=1, liga=1）",value:EditorJSON.objects(text["features"]).map { "\(EditorJSON.string($0["tag"]))=\(Int(EditorJSON.number($0["value"])))" }.joined(separator:", "),disabled:disabled) { value in
                let parts=value.trimmingCharacters(in:.whitespaces).isEmpty ? []:value.split(separator:",",omittingEmptySubsequences:false)
                var features:[[String:Any]]=[]
                for part in parts { let items=part.split(separator:"=",omittingEmptySubsequences:false);guard items.count==2,let number=Int(items[1].trimmingCharacters(in:.whitespaces)),number>=0,number<=65535 else { featureError="機能は tag=整数 の形式で 0〜65535 を指定してください。";return };features.append(["tag":items[0].trimmingCharacters(in:.whitespaces),"value":number]) }
                featureError=""
                onStyle(["features":features])
            }
            if !featureError.isEmpty { Text(featureError).font(.caption).foregroundStyle(.red) }
            Text("部分スタイルは先頭を 0 とした文字範囲に設定します。").font(.caption).foregroundStyle(.secondary)
            HStack { NativeNumberField("開始位置",value:start,min:0,max:Double(characters.count),integer:true,disabled:disabled) { start=$0 };NativeNumberField("終了位置",value:end,min:0,max:Double(characters.count),integer:true,disabled:disabled) { end=$0 } }
            NativeChoice("範囲のフォント",value:font.isEmpty ? EditorJSON.string(text["fontId"]):font,options:fonts.map { (EditorJSON.string($0["id"]),EditorJSON.string($0["nameHint"])) },disabled:disabled) { font=$0 }
            NativeColorField("範囲の色",hex:color,disabled:disabled) { color=$0 }
            Button("部分スタイルを追加") {
                let from=String(characters.prefix(Int(start))).utf8.count,to=String(characters.prefix(Int(end))).utf8.count
                let next=runs+[["start":from,"end":to,"fontId":font.isEmpty ? EditorJSON.string(text["fontId"]):font,"color":color]]
                onStyle(["runs":next.sorted { EditorJSON.number($0["start"])<EditorJSON.number($1["start"]) }])
            }.disabled(disabled || start>=end || end>Double(characters.count))
            ForEach(runs.indices,id:\.self) { index in HStack { Text("\(Int(EditorJSON.number(runs[index]["start"])))–\(Int(EditorJSON.number(runs[index]["end"]))) bytes · \(EditorJSON.string(runs[index]["fontId"]))").font(.caption);Spacer();Button("解除") { var next=runs;next.remove(at:index);onStyle(["runs":next]) }.disabled(disabled) } }
        }.onAppear { end=Double(characters.count);font=EditorJSON.string(text["fontId"]) }
    }
}

private struct NativePathEditor:View {
    var node:[String:Any];var disabled:Bool;var onCommand:([String:Any])->Void
    @State private var error=""
    private var parsed:EditorPath? { try? EditorPath(EditorJSON.string(node["data"])) }
    private var id:String { EditorJSON.string(node["id"]) }
    var body:some View {
        InspectorSection("ベジェパスの点と制御点") {
            NativeCommitText("元のパスデータ",value:EditorJSON.string(node["data"]),multiline:true,disabled:disabled) { data in do { _=try EditorPath(data);onCommand(["op":"setPathData","nodeId":id,"data":data]);error="" } catch { self.error=error.localizedDescription } }
            NativeChoice("塗り規則",value:EditorJSON.string(node["fillRule"],"non-zero"),options:[("non-zero","非ゼロ"),("even-odd","偶奇（穴のある複合パス）")],disabled:disabled) { onCommand(["op":"setPathFillRule","nodeId":id,"fillRule":$0]) }
            if let path=parsed {
                ForEach(Array(path.segments.prefix(250).enumerated()),id:\.offset) { index,segment in
                    VStack(alignment:.leading,spacing:6) {
                        HStack { Text("\(index+1). \(String(segment.command))").font(.caption.bold());Spacer();Button("削除") { change { path in if path.segments[index].command=="M" { let end=path.segments[(index+1)...].firstIndex { $0.command=="M" } ?? path.segments.count;path.segments.removeSubrange(index..<end) } else { path.segments.remove(at:index) } } }.disabled(disabled || (segment.command=="M" && path.segments.filter { $0.command=="M" }.count==1)) }
                        ForEach(segment.points.indices,id:\.self) { point in HStack { NativeNumberField("\(point+1) X",value:segment.points[point].x,min:-1_000_000,max:1_000_000,disabled:disabled) { value in change { $0.segments[index].points[point].x=value } };NativeNumberField("\(point+1) Y",value:segment.points[point].y,min:-1_000_000,max:1_000_000,disabled:disabled) { value in change { $0.segments[index].points[point].y=value } } } }
                    }.padding(7).background(Color.primary.opacity(0.04)).cornerRadius(5)
                }
                if path.segments.count>250 { Text("数値一覧は先頭 250 件です。全点は元データで編集できます。").font(.caption).foregroundStyle(.secondary) }
                HStack { ForEach(["L","Q","C","M","Z"],id:\.self) { kind in Button(["L":"直線","Q":"二次","C":"三次","M":"別パス","Z":"閉じる"][kind]!) { append(Character(kind)) }.disabled(disabled) } }.font(.caption)
            } else { Text("数値編集の対象外です。元データを保持しています。").font(.caption).foregroundStyle(.orange) }
            if !error.isEmpty { Text(error).font(.caption).foregroundStyle(.red) }
            let box=EditorBounds(node["viewBox"])
            ForEach(["x","y","width","height"],id:\.self) { key in NativeNumberField("viewBox \(key)",value:EditorJSON.number(box.raw[key]),min:key=="width" || key=="height" ? 0.001:-1_000_000,max:key=="width" || key=="height" ? 16384:1_000_000,disabled:disabled) { value in var next=box.raw;next[key]=value;onCommand(["op":"setPathViewBox","nodeId":id,"viewBox":next]) } }
        }
    }
    private func change(_ update:(inout EditorPath)->Void) { guard var path=parsed else { return };update(&path);do { let data=try path.serialized();onCommand(["op":"setPathData","nodeId":id,"data":data]);error="" } catch { self.error=error.localizedDescription } }
    private func append(_ command:Character) {
        change { path in var current=CGPoint.zero,start=CGPoint.zero;for segment in path.segments { if segment.command=="M" { start=segment.points[0] };if let point=segment.points.last { current=point } else if segment.command=="Z" { current=start } };let end=CGPoint(x:current.x+40,y:current.y);let points:[CGPoint]=command=="M" ? [CGPoint(x:current.x+20,y:current.y+20)]:command=="L" ? [end]:command=="Q" ? [CGPoint(x:current.x+20,y:current.y-20),end]:command=="C" ? [CGPoint(x:current.x+12,y:current.y-20),CGPoint(x:current.x+28,y:current.y-20),end]:[];path.segments.append(EditorPathSegment(command:command,points:points)) }
    }
}

private struct NativeBrandEditor:View {
    @ObservedObject var model:StudioModel;var node:[String:Any]?;var disabled:Bool
    var document:Document?;var context:EditorCommitContext
    private var brand:[String:Any] { document?.brandSnapshot ?? ["id":String(("brand-"+EditorJSON.string(document?.raw["documentId"])).prefix(64)),"revision":1,"colors":[[String:Any]](),"textStyles":[[String:Any]](),"logoAssetIds":Array(Set(document?.nodes.filter { EditorJSON.string($0["type"])=="logo" }.map { EditorJSON.string($0["assetId"]) } ?? []))] }
    private var colors:[[String:Any]] { EditorJSON.objects(brand["colors"]) }
    private var textStyles:[[String:Any]] { EditorJSON.objects(brand["textStyles"]) }
    var body:some View {
        InspectorSection("素材とブランド") {
            Text("色・文字スタイル・許可ロゴを作品内に固定します。").font(.caption).foregroundStyle(.secondary)
            ForEach(colors.indices,id:\.self) { index in
                let item=colors[index]
                NativeCommitText("色の名前",value:EditorJSON.string(item["name"]),disabled:disabled) { value in var next=colors;next[index]["name"]=value;update(["colors":next]) }
                NativeColorField("カラー",hex:EditorJSON.string(item["color"]),disabled:disabled) { value in var next=colors;next[index]["color"]=value;update(["colors":next]) }
                HStack { Button("選択レイヤーへ適用") { if let node=node { let id=EditorJSON.string(node["id"]),color=EditorJSON.string(item["color"]);send(EditorJSON.string(node["type"])=="text" ? ["op":"setTextStyle","nodeId":id,"style":["color":color]]:["op":"setFill","nodeId":id,"fill":["kind":"solid","color":color]]) } }.disabled(disabled || !["text","rect","path"].contains(EditorJSON.string(node?["type"])));Button("削除") { var next=colors;next.remove(at:index);update(["colors":next]) }.disabled(disabled) }.font(.caption)
            }
            Button("名前付きカラーを追加") { update(["colors":colors+[["id":"color-"+UUID().uuidString.lowercased().prefix(8),"name":"カラー \(colors.count+1)","color":"#39735b"]]]) }.disabled(disabled || colors.count>=100)
            ForEach(textStyles.indices,id:\.self) { index in
                let item=textStyles[index]
                NativeCommitText("文字スタイル名",value:EditorJSON.string(item["name"]),disabled:disabled) { value in var next=textStyles;next[index]["name"]=value;update(["textStyles":next]) }
                HStack { Button("選択文字へ適用") { if let node=node { var style=EditorJSON.object(item["style"]);style.removeValue(forKey:"value");style["runs"]=[[String:Any]]();send(["op":"setTextStyle","nodeId":EditorJSON.string(node["id"]),"style":style]) } }.disabled(disabled || EditorJSON.string(node?["type"]) != "text");Button("削除") { var next=textStyles;next.remove(at:index);update(["textStyles":next]) }.disabled(disabled) }
            }
            Button("選択中の文字スタイルを登録") { if let node=node { var style=EditorJSON.object(node["text"]);style["runs"]=[[String:Any]]();update(["textStyles":textStyles+[["id":"style-"+UUID().uuidString.lowercased().prefix(8),"name":"文字スタイル \(textStyles.count+1)","style":style]]]) } }.disabled(disabled || EditorJSON.string(node?["type"]) != "text" || textStyles.count>=100)
            ForEach((document?.assets ?? []).indices,id:\.self) { index in let asset=document!.assets[index],id=EditorJSON.string(asset["id"]);Toggle("許可ロゴ: \(id)",isOn:Binding(get:{EditorJSON.strings(brand["logoAssetIds"]).contains(id)},set:{value in var ids=EditorJSON.strings(brand["logoAssetIds"]);if value { ids.append(id) } else { ids.removeAll { $0==id } };update(["logoAssetIds":ids])})).disabled(disabled) }
        }
    }
    private func send(_ command:[String:Any]) { Task { await model.edit(command,context:context) } }
    private func update(_ fields:[String:Any]) { var next=brand;for (key,value) in fields { next[key]=value };next["revision"]=EditorJSON.number(brand["revision"],1)+1;send(["op":"setBrandSnapshot","brandSnapshot":next]) }
}
