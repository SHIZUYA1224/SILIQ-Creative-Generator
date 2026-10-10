import SwiftUI

let studioGreen = Color(red:0.20,green:0.36,blue:0.28)

struct WorkspaceView: View {
    @ObservedObject var model:StudioModel
    var body: some View {
        VStack(spacing:0) {
            HStack(spacing:18) {
                Text("S").font(.system(size:26,weight:.bold,design:.rounded)).foregroundColor(.white).frame(width:42,height:42).background(studioGreen).cornerRadius(12)
                VStack(alignment:.leading,spacing:2) {Text("SILIQ").font(.system(size:17,weight:.bold));Text("CREATIVE GENERATOR").font(.system(size:8,weight:.medium)).tracking(2).foregroundColor(.secondary)}
                Divider().frame(height:28)
                VStack(alignment:.leading,spacing:3) {Text(model.session?.document.title ?? "起動しています").font(.system(size:14,weight:.semibold));Text(model.savedRevision==model.session?.document.revision ? "保存済み · ローカルプロジェクト":"未保存の変更 · ローカルプロジェクト").font(.system(size:10)).foregroundColor(.secondary)}
                Spacer()
                Label("完全オフライン",systemImage:"shield.checkered").font(.system(size:11)).foregroundColor(studioGreen)
                Button {model.showStorage=true} label:{Image(systemName:"gearshape")}.help("保存領域と復旧")
                Button("保存") {Task{await model.saveProject()}}.disabled(model.busy || model.session==nil)
                Button("書き出す") {Task{await model.beginExport()}}.buttonStyle(.borderedProminent).tint(studioGreen).disabled(model.busy || model.session==nil)
            }.padding(.horizontal,24).padding(.vertical,16).background(.white)
            Divider()
            HStack(spacing:16) {
                Button("デザイン制作") {model.showBatch=false}.buttonStyle(.plain).font(.system(size:12,weight:.semibold)).foregroundColor(studioGreen)
                Button("データから量産") {Task{await model.beginBatch()}}.buttonStyle(.plain).font(.system(size:12)).disabled(model.busy || model.session==nil)
                Picker("編集モード",selection:Binding(get:{model.session?.mode ?? "authoring"},set:{mode in Task{await model.setMode(mode)}})){Text("制作").tag("authoring");Text("差し替え").tag("production")}.pickerStyle(.segmented).frame(width:170).help("差し替えでは宣言した文字・画像スロットだけを変更できます").disabled(model.busy || model.session==nil)
                Spacer()
                Button("テンプレート") {model.showTemplates=true}
                Button("新規") {Task{await model.newDocument()}}.disabled(model.busy)
                Button("開く") {Task{await model.openProject()}}.disabled(model.busy)
            }.padding(.horizontal,24).padding(.vertical,11).background(Color(red:0.98,green:0.99,blue:0.96))
            Divider()
            if model.session != nil {EditorView(model:model)}
            else {VStack(spacing:16){ProgressView();Text(model.issues.isEmpty ? "制作エンジンを準備しています…":"前回の作品を復旧できませんでした。新規または開くから制作できます。")}.frame(maxWidth:.infinity,maxHeight:.infinity)}
            if !model.issues.isEmpty {IssuePanel(issues:model.issues).frame(maxHeight:160)}
            Divider()
            HStack {
                Label(model.issues.isEmpty ? "検査エラーなし":"\(model.issues.filter{$0.severity=="error"}.count) 件のエラー",systemImage:model.issues.isEmpty ? "checkmark.circle":"exclamationmark.circle").foregroundColor(model.issues.isEmpty ? studioGreen:.orange)
                Spacer()
                if let revision=model.session?.document.revision {Text("revision \(revision) · sRGB")}
                Spacer()
                Text(model.busy ? "処理中…":model.notice.isEmpty ? "素材を外部へ送信しません":model.notice)
            }.font(.system(size:10)).padding(.horizontal,20).padding(.vertical,8).background(Color(red:0.97,green:0.98,blue:0.95))
        }.background(Color(red:0.97,green:0.98,blue:0.95))
        .sheet(isPresented:$model.showTemplates){TemplateGallery(model:model)}
        .sheet(isPresented:$model.showExport){ExportView(model:model)}
        .sheet(isPresented:$model.showBatch){BatchView(model:model)}
        .sheet(isPresented:$model.showStorage){StorageView(model:model)}
    }
}

struct TemplateGallery:View {
    @ObservedObject var model:StudioModel
    let templates:[(String,String,String,Color)]=[
        ("retail-promo-template","店舗・キャンペーン広告","広告 / SNS",Color(red:0.82,green:0.26,blue:0.16)),
        ("showcase","ブランド・商品紹介","SNS / 商品",Color(red:0.16,green:0.34,blue:0.34)),
        ("blog-summary","記事の図解・3つのポイント","ブログ / 図解",studioGreen),
        ("blog-comparison","比較表・説明資料","ブログ / PDF",Color(red:0.45,green:0.53,blue:0.38)),
        ("basic","自由なレイアウト","編集の基本",Color(red:0.39,green:0.49,blue:0.61))]
    var body:some View {
        VStack(alignment:.leading,spacing:24) {
            HStack {VStack(alignment:.leading,spacing:8){Text("START WITH A TEMPLATE").font(.system(size:10,weight:.medium)).tracking(2).foregroundColor(.secondary);Text("アイデアを、かたちに。").font(.system(size:24,weight:.semibold));Text("広告、SNS、記事の図解。編集できるテンプレートから。").font(.system(size:12)).foregroundColor(.secondary)};Spacer();Button("閉じる"){model.showTemplates=false}.keyboardShortcut(.cancelAction)}
            LazyVGrid(columns:[GridItem(.flexible()),GridItem(.flexible()),GridItem(.flexible())],spacing:18) {
                ForEach(templates,id:\.0){template in
                    Button {Task{await model.openTemplate(template.0)}} label:{VStack(alignment:.leading,spacing:8){ZStack(alignment:.topLeading){template.3;VStack(alignment:.leading,spacing:24){Text("SILIQ").font(.system(size:10,weight:.bold)).tracking(2);Text(template.1).font(.system(size:21,weight:.semibold)).lineLimit(3)}}.foregroundColor(.white).padding(22).frame(height:150).cornerRadius(8);Text(template.2).font(.system(size:10)).foregroundColor(.secondary);Text(template.1).font(.system(size:12,weight:.semibold));Text("テンプレートを開く →").font(.system(size:10)).foregroundColor(studioGreen)}.contentShape(Rectangle())}.buttonStyle(.plain).disabled(model.busy)
                }
            }
        }.padding(30).frame(width:850)
    }
}

struct ExportView:View {
    @ObservedObject var model:StudioModel
    @State private var format="png"
    @State private var quality=90.0
    @State private var dpi=144.0
    @State private var background=Color.white
    @State private var raster=false
    var needsRaster:Bool {format=="pdf" || format=="svg"}
    var body:some View {
        VStack(alignment:.leading,spacing:18) {
            HStack{Text("作品を書き出す").font(.title2.bold());Spacer();Button("閉じる"){model.showExport=false}.keyboardShortcut(.cancelAction)}
            if let variant=model.variant {Text("\(jsonString(variant["label"])) · \(Int(jsonNumber(variant["width"]))) × \(Int(jsonNumber(variant["height"]))) px · sRGB").font(.system(size:12)).foregroundColor(.secondary)}
            Picker("出力形式",selection:$format){Text("PNG（透過対応）").tag("png");Text("JPEG（背景色を指定）").tag("jpeg");Text("WebP（ロスレス）").tag("webp");Text("PDF（説明資料・RGB校正）").tag("pdf");Text("SVG（画像を内包）").tag("svg")}.onChange(of:format){_ in raster=false}
            if format=="jpeg" {Slider(value:$quality,in:1...100,step:1){Text("品質 \(Int(quality))")};ColorPicker("透過部分の背景色",selection:$background,supportsOpacity:false)}
            if format=="pdf" {HStack{Text("PDF 解像度 DPI");TextField("72–1200",value:$dpi,formatter:studioNumberFormatter).frame(width:90)}}
            if needsRaster {VStack(alignment:.leading,spacing:8){Text("アートボード全体を画像として内包します。文字・図形は編集できません。PDFはRGB校正用です。").font(.system(size:11));Toggle("この画像化を許可する",isOn:$raster)}.padding(14).background(Color.orange.opacity(0.08)).cornerRadius(8)}
            if jsonString(model.variant?["approvalStatus"]) != "approved" {Text("未承認です。制作画面でアートボードを承認してください。").foregroundColor(.orange).font(.system(size:11))}
            Button("保存先を選んで書き出す") {Task{await export()}}.buttonStyle(.borderedProminent).tint(studioGreen).disabled(model.busy || jsonString(model.variant?["approvalStatus"]) != "approved" || needsRaster && !raster || format=="pdf" && (!dpi.isFinite || dpi<72 || dpi>1200 || dpi.rounded() != dpi))
            Text("同じ描画エンジンから出力します。既存の出力ファイルへの上書きは拒否します。").font(.system(size:10)).foregroundColor(.secondary)
        }.padding(28).frame(width:480)
    }
    func export() async {
        let fileExtension=format=="jpeg" ? "jpg":format
        guard let session=model.session,let url=await model.saveFile(name:"\(session.document.title).\(fileExtension)",extension:fileExtension) else{return}
        await model.perform {
            try await model.withAccess(url:url) {
                var recipe:[String:Any]=["format":format,"quality":NSNull(),"jpegBackground":NSNull(),"webpLossless":true,"allowRasterization":raster,"pdfDpi":format=="pdf" ? Int(dpi):144]
                if format=="jpeg" {recipe["quality"]=Int(quality);recipe["jpegBackground"]=studioHex(background)}
                _=try await model.core.call("export_design_path",["sessionId":session.sessionId,"revision":session.document.revision,"variantId":model.variantID,"path":url.path,"recipe":recipe])
                model.notice="作品を書き出しました";model.showExport=false
            }
        }
    }
}
let studioNumberFormatter:NumberFormatter={let formatter=NumberFormatter();formatter.numberStyle = .decimal;formatter.maximumFractionDigits=2;formatter.usesGroupingSeparator=false;return formatter}()
func studioHex(_ color:Color)->String {let converted=NSColor(color).usingColorSpace(.sRGB) ?? .white;return String(format:"#%02X%02X%02X",Int((converted.redComponent*255).rounded()),Int((converted.greenComponent*255).rounded()),Int((converted.blueComponent*255).rounded()))}

struct IssuePanel:View {
    let issues:[StudioIssue]
    var body:some View {ScrollView{VStack(alignment:.leading,spacing:10){Text("書き出し前に確認してください").font(.system(size:11,weight:.semibold));ForEach(issues){issue in VStack(alignment:.leading,spacing:4){Text(issue.message).font(.system(size:11));Text("\(issue.code) · \(issue.remedy)").font(.system(size:9)).foregroundColor(.secondary)}}}.frame(maxWidth:.infinity,alignment:.leading).padding(14)}.background(Color.orange.opacity(0.08))}
}
