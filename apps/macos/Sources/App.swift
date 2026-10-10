import SwiftUI

@main struct SILIQStudioApp: App {
    @StateObject private var model = StudioModel()
    var body: some Scene {
        WindowGroup("SILIQ Creative Generator") {
            WorkspaceView(model:model).preferredColorScheme(.light).tint(studioGreen).frame(minWidth:1040,minHeight:720).task {await model.start()}
        }
        .defaultSize(width:1440,height:960)
        .commands {
            CommandGroup(replacing:.newItem) {
                Button("新規作品") {Task{await model.newDocument()}}.keyboardShortcut("n").disabled(model.busy)
                Button("プロジェクトを開く…") {Task{await model.openProject()}}.keyboardShortcut("o").disabled(model.busy)
                Button("保存…") {Task{await model.saveProject()}}.keyboardShortcut("s").disabled(model.busy || model.session==nil)
                Button("別名で保存…") {Task{await model.saveProject(saveAs:true)}}.keyboardShortcut("s",modifiers:[.command,.shift]).disabled(model.busy || model.session==nil)
            }
            CommandGroup(replacing:.undoRedo) {
                Button("元に戻す") {Task{await model.history("undo")}}.keyboardShortcut("z").disabled(model.busy || model.session==nil)
                Button("やり直す") {Task{await model.history("redo")}}.keyboardShortcut("z",modifiers:[.command,.shift]).disabled(model.busy || model.session==nil)
            }
            CommandMenu("制作") {
                Button("書き出す…") {Task{await model.beginExport()}}.keyboardShortcut("e").disabled(model.busy || model.session==nil)
                Button("画像を取り込む…") {Task{await model.importAsset("raster",addToCanvas:true)}}.keyboardShortcut("i").disabled(model.busy || !model.authoring)
                Button("テンプレート") {model.showTemplates=true}
                Button("データから量産") {Task{await model.beginBatch()}}.disabled(model.busy || model.session==nil)
                Button("保存領域と復旧") {model.showStorage=true}
            }
        }
    }
}
