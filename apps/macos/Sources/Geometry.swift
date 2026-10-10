import Foundation
import CoreGraphics

enum EditorJSON {
    static func object(_ value: Any?) -> [String: Any] { value as? [String: Any] ?? [:] }
    static func objects(_ value: Any?) -> [[String: Any]] { value as? [[String: Any]] ?? [] }
    static func string(_ value: Any?, _ fallback: String = "") -> String { value as? String ?? fallback }
    static func number(_ value: Any?, _ fallback: Double = 0) -> Double { (value as? NSNumber)?.doubleValue ?? fallback }
    static func bool(_ value: Any?, _ fallback: Bool = false) -> Bool { value as? Bool ?? fallback }
    static func strings(_ value: Any?) -> [String] { value as? [String] ?? [] }
    static func equal(_ a: [String: Any], _ b: [String: Any]) -> Bool { NSDictionary(dictionary: a).isEqual(to: b) }
}

// A staged input belongs to the document, artboard and node that created it.
// Selection can change before AppKit calls the old input's blur/dismantle hook.
struct EditorCommitContext: Equatable {
    let sessionID: String
    let variantID: String
    let nodeID: String
    func matches(sessionID: String, variantID: String) -> Bool {
        self.sessionID == sessionID && self.variantID == variantID
    }
    func nodeCommand(_ op: String, fields: [String: Any]) -> [String: Any] {
        var command = fields; command["op"] = op
        if !nodeID.isEmpty { command["nodeId"] = nodeID }
        return command
    }
}

struct EditorBounds: Equatable {
    var x: Double; var y: Double; var width: Double; var height: Double
    init(_ value: Any?) {
        let raw = EditorJSON.object(value)
        x = EditorJSON.number(raw["x"]); y = EditorJSON.number(raw["y"])
        width = EditorJSON.number(raw["width"], 1); height = EditorJSON.number(raw["height"], 1)
    }
    init(x: Double, y: Double, width: Double, height: Double) { self.x=x; self.y=y; self.width=width; self.height=height }
    var raw: [String: Any] { ["x":x,"y":y,"width":width,"height":height] }
    var corners: [CGPoint] { [CGPoint(x:x,y:y),CGPoint(x:x+width,y:y),CGPoint(x:x+width,y:y+height),CGPoint(x:x,y:y+height)] }
    func moved(_ dx: Double, _ dy: Double) -> EditorBounds { EditorBounds(x:x+dx,y:y+dy,width:width,height:height) }
}

struct EditorMatrix {
    var a: Double=1; var b: Double=0; var c: Double=0; var d: Double=1; var e: Double=0; var f: Double=0
    init() {}
    init(_ values: Any?) {
        guard let values=values as? [NSNumber],values.count==6 else { return }
        a=values[0].doubleValue;b=values[1].doubleValue;c=values[2].doubleValue;d=values[3].doubleValue;e=values[4].doubleValue;f=values[5].doubleValue
    }
    init(a:Double,b:Double,c:Double,d:Double,e:Double,f:Double) { self.a=a;self.b=b;self.c=c;self.d=d;self.e=e;self.f=f }
    var raw:[Double] { [a,b,c,d,e,f] }
    func times(_ rhs: EditorMatrix) -> EditorMatrix {
        EditorMatrix(a:a*rhs.a+c*rhs.b,b:b*rhs.a+d*rhs.b,c:a*rhs.c+c*rhs.d,d:b*rhs.c+d*rhs.d,e:a*rhs.e+c*rhs.f+e,f:b*rhs.e+d*rhs.f+f)
    }
    func point(_ point: CGPoint) -> CGPoint { CGPoint(x:a*point.x+c*point.y+e,y:b*point.x+d*point.y+f) }
    func inverseVector(dx: Double, dy: Double) -> (dx:Double,dy:Double) {
        let det=a*d-b*c
        guard det.isFinite,abs(det)>=1e-12 else { return (0,0) }
        return ((d*dx-c*dy)/det,(a*dy-b*dx)/det)
    }
    static func translation(_ x:Double,_ y:Double) -> EditorMatrix { EditorMatrix(a:1,b:0,c:0,d:1,e:x,f:y) }
}

enum EditorGeometry {
    static func resolvedNodes(_ document:[String:Any], variantID:String) -> [[String:Any]] {
        let variant=EditorJSON.objects(document["variants"]).first { EditorJSON.string($0["id"])==variantID }
        let patches=Dictionary(uniqueKeysWithValues:EditorJSON.objects(variant?["nodePatches"]).map { (EditorJSON.string($0["nodeId"]),$0) })
        return EditorJSON.objects(document["nodes"]).map { original in
            guard let patch=patches[EditorJSON.string(original["id"])] else { return original }
            var node=original;node["bounds"]=patch["bounds"]
            if EditorJSON.string(node["type"])=="text" {
                var text=EditorJSON.object(node["text"])
                for key in ["fontSize","lineHeight"] { if let value=patch[key] { text[key]=value } }
                node["text"]=text
            }
            return node
        }
    }
    static func resolvedStyles(_ document:[String:Any], variantID:String) -> [[String:Any]] {
        var styles=EditorJSON.objects(document["nodeStyles"])
        let variant=EditorJSON.objects(document["variants"]).first { EditorJSON.string($0["id"])==variantID }
        for patch in EditorJSON.objects(variant?["nodePatches"]) {
            guard let affine=patch["affine"] else { continue }
            let id=EditorJSON.string(patch["nodeId"])
            if let index=styles.firstIndex(where:{EditorJSON.string($0["nodeId"])==id}) { styles[index]["affine"]=affine }
            else { styles.append(["nodeId":id,"affine":affine]) }
        }
        return styles
    }
    static func ancestorIDs(_ nodes:[[String:Any]], id:String) -> [String] {
        var parents:[String:String]=[:]
        for node in nodes where EditorJSON.string(node["type"])=="group" {
            for child in EditorJSON.strings(node["childIds"]) { parents[child]=EditorJSON.string(node["id"]) }
        }
        var result:[String]=[],current=parents[id]
        while let next=current,result.count<32,!result.contains(next) { result.append(next);current=parents[next] }
        return result
    }
    static func locked(_ nodes:[[String:Any]],node:[String:Any]) -> Bool {
        EditorJSON.bool(node["locked"]) || ancestorIDs(nodes,id:EditorJSON.string(node["id"])).contains { id in
            nodes.first { EditorJSON.string($0["id"])==id }.map { EditorJSON.bool($0["locked"]) } ?? false
        }
    }
    static func rotation(_ node:[String:Any]) -> EditorMatrix {
        let bounds=EditorBounds(node["bounds"]),radians=EditorJSON.number(node["rotationDeg"])*Double.pi/180
        let c=cos(radians),s=sin(radians),cx=bounds.x+bounds.width/2,cy=bounds.y+bounds.height/2
        return EditorMatrix(a:c,b:s,c:-s,d:c,e:cx-c*cx+s*cy,f:cy-s*cx-c*cy)
    }
    static func transform(_ node:[String:Any],styles:[[String:Any]]) -> EditorMatrix {
        let style=styles.first { EditorJSON.string($0["nodeId"])==EditorJSON.string(node["id"]) }
        return rotation(node).times(EditorMatrix(style?["affine"]))
    }
    static func parentTransform(_ nodes:[[String:Any]],node:[String:Any],styles:[[String:Any]]) -> EditorMatrix {
        var result=EditorMatrix()
        for id in ancestorIDs(nodes,id:EditorJSON.string(node["id"])).reversed() {
            if let parent=nodes.first(where:{EditorJSON.string($0["id"])==id}) { result=result.times(transform(parent,styles:styles)) }
        }
        return result
    }
    static func frame(_ nodes:[[String:Any]],node:[String:Any],styles:[[String:Any]]) -> EditorMatrix {
        parentTransform(nodes,node:node,styles:styles).times(EditorJSON.string(node["type"])=="group" ? rotation(node) : transform(node,styles:styles))
    }
    static func resized(_ node:[String:Any],dw:Double,dh:Double) -> EditorBounds {
        let b=EditorBounds(node["bounds"]),width=max(1,min(16384,b.width+dw)),height=max(1,min(16384,b.height+dh))
        let dx=width-b.width,dy=height-b.height,r=EditorJSON.number(node["rotationDeg"])*Double.pi/180,c=cos(r),s=sin(r)
        return EditorBounds(x:b.x+(c-1)*dx/2-s*dy/2,y:b.y+s*dx/2+(c-1)*dy/2,width:width,height:height)
    }
    static func groupAffine(before:EditorBounds,after:EditorBounds,affine:Any?) -> EditorMatrix {
        let sx=after.width/before.width,sy=after.height/before.height
        return EditorMatrix(a:sx,b:0,c:0,d:sy,e:after.x-sx*before.x,f:after.y-sy*before.y).times(EditorMatrix(affine))
    }
    static func boundsCommand(_ document:[String:Any],variantID:String,changes:[[String:Any]]) -> [String:Any] {
        guard var variant=EditorJSON.objects(document["variants"]).first(where:{EditorJSON.string($0["id"])==variantID}),!EditorJSON.objects(variant["nodePatches"]).isEmpty else {
            return ["op":"batch","commands":changes.map { ["op":"setBounds","nodeId":EditorJSON.string($0["id"]),"bounds":EditorJSON.object($0["bounds"])] as [String:Any] }]
        }
        let nodes=resolvedNodes(document,variantID:variantID),styles=resolvedStyles(document,variantID:variantID)
        var patches=EditorJSON.objects(variant["nodePatches"])
        for change in changes {
            let id=EditorJSON.string(change["id"]),index=patches.firstIndex { EditorJSON.string($0["nodeId"])==id }
            var patch=index.map { patches[$0] } ?? ["nodeId":id]
            patch["bounds"]=change["bounds"]
            if let node=nodes.first(where:{EditorJSON.string($0["id"])==id}),EditorJSON.string(node["type"])=="group" {
                patch["affine"]=groupAffine(before:EditorBounds(node["bounds"]),after:EditorBounds(change["bounds"]),affine:styles.first { EditorJSON.string($0["nodeId"])==id }?["affine"]).raw
            }
            if let index=index { patches[index]=patch } else { patches.append(patch) }
        }
        variant["nodePatches"]=patches;variant["approvalStatus"]="draft"
        return ["op":"setVariant","variant":variant]
    }
    static func tree(_ nodes:[[String:Any]],rootID:String) -> [(node:[String:Any],depth:Int)] {
        var result:[(node:[String:Any],depth:Int)]=[],seen=Set<String>()
        func visit(_ id:String,_ depth:Int) {
            guard !seen.contains(id),seen.count<2000,let node=nodes.first(where:{EditorJSON.string($0["id"])==id}) else { return }
            seen.insert(id);result.append((node,depth))
            if EditorJSON.string(node["type"])=="group" { for child in EditorJSON.strings(node["childIds"]).reversed() { visit(child,depth+1) } }
        }
        if let root=nodes.first(where:{EditorJSON.string($0["id"])==rootID}) { for child in EditorJSON.strings(root["childIds"]).reversed() { visit(child,0) } }
        return result
    }
    static func targets(_ nodes:[[String:Any]],rootID:String,selected:[String]) -> [[String:Any]] {
        var result:[[String:Any]]=[],seen=Set<String>()
        func visit(_ id:String) {
            guard !seen.contains(id),seen.count<2000,let node=nodes.first(where:{EditorJSON.string($0["id"])==id}),EditorJSON.bool(node["visible"],true) else { return }
            seen.insert(id)
            if EditorJSON.string(node["type"])=="group" { EditorJSON.strings(node["childIds"]).forEach(visit) }
            else { result.append(node) }
        }
        visit(rootID)
        result += nodes.filter { node in
            let id=EditorJSON.string(node["id"])
            return id != rootID && EditorJSON.string(node["type"])=="group" && selected.contains(id) && EditorJSON.bool(node["visible"],true) && !ancestorIDs(nodes,id:id).contains { parent in
                !EditorJSON.bool(nodes.first { EditorJSON.string($0["id"])==parent }?["visible"],true)
            }
        }
        return result
    }
    static func duplicateCommand(_ document:[String:Any],selected:[String]) -> (command:[String:Any],selected:[String])? {
        let nodes=EditorJSON.objects(document["nodes"]),root=EditorJSON.string(EditorJSON.object(document["baseArtboard"])["rootNodeId"])
        let top=selected.filter { id in id != root && nodes.contains { EditorJSON.string($0["id"])==id } && !ancestorIDs(nodes,id:id).contains { selected.contains($0) } }
        guard !top.isEmpty else { return nil }
        var source:[[String:Any]]=[]
        func collect(_ id:String) { if let node=nodes.first(where:{EditorJSON.string($0["id"])==id}) { source.append(node);EditorJSON.strings(node["childIds"]).forEach(collect) } }
        top.forEach(collect)
        guard nodes.count+source.count<=2000 else { return nil }
        let ids=Dictionary(uniqueKeysWithValues:source.map { (EditorJSON.string($0["id"]),"copy-"+UUID().uuidString.lowercased().prefix(8)) })
        var commands:[[String:Any]]=[],parents:[String:String]=[:],siblings:[String:[String]]=[:]
        for node in nodes where EditorJSON.string(node["type"])=="group" { let id=EditorJSON.string(node["id"]);siblings[id]=EditorJSON.strings(node["childIds"]);for child in siblings[id]! { parents[child]=id } }
        func create(_ id:String,_ parent:String,_ index:Int,_ moved:Bool) {
            guard let original=nodes.first(where:{EditorJSON.string($0["id"])==id}),let copyID=ids[id] else { return }
            var copy=original;copy["id"]=copyID;copy["name"]=EditorJSON.string(copy["name"])+" コピー";copy["locked"]=false
            if moved { copy["bounds"]=EditorBounds(copy["bounds"]).moved(20,20).raw }
            if EditorJSON.string(copy["type"])=="group" { copy["childIds"]=[String]() }
            commands.append(["op":"addNode","parentId":parent,"index":index,"node":copy])
            for (i,child) in EditorJSON.strings(original["childIds"]).enumerated() { create(child,copyID,i,false) }
        }
        for id in top { if let parent=parents[id],let index=siblings[parent]?.firstIndex(of:id) { create(id,parent,index+1,true);siblings[parent]!.insert(ids[id]!,at:index+1) } }
        let styles=EditorJSON.objects(document["nodeStyles"])
        for node in source {
            let id=EditorJSON.string(node["id"]),isGroup=EditorJSON.string(node["type"])=="group",original=styles.first { EditorJSON.string($0["nodeId"])==id }
            guard original != nil || (top.contains(id) && isGroup) else { continue }
            var style=original ?? [:];style["nodeId"]=ids[id]!
            if top.contains(id) {
                if isGroup { style["affine"]=EditorMatrix.translation(20,20).times(EditorMatrix(original?["affine"])).raw }
                else {
                    if original?["affine"] != nil { style["affine"]=EditorMatrix.translation(20,20).times(EditorMatrix(original?["affine"])).times(.translation(-20,-20)).raw }
                    for key in ["clip","mask"] {
                        if var value=style[key] as? [String:Any] {
                            guard let path=try? EditorPath(EditorJSON.string(value["data"])),let moved=try? path.translated(20,20).serialized() else { return nil }
                            value["data"]=moved;style[key]=value
                        }
                    }
                }
            }
            if let targets=style["protectedFrom"] as? [String] { style["protectedFrom"]=targets.map { ids[$0] ?? $0 } }
            commands.append(["op":"setNodeStyle","nodeId":ids[id]!,"style":style])
        }
        for original in EditorJSON.objects(document["variants"]) {
            var copies:[[String:Any]]=[]
            for patch in EditorJSON.objects(original["nodePatches"]) {
                let id=EditorJSON.string(patch["nodeId"]);guard let copyID=ids[id] else { continue }
                var copy=patch;copy["nodeId"]=copyID
                if top.contains(id) {
                    copy["bounds"]=EditorBounds(patch["bounds"]).moved(20,20).raw
                    if nodes.first(where:{EditorJSON.string($0["id"])==id}).map({EditorJSON.string($0["type"])=="group"}) ?? false { copy["affine"]=EditorMatrix.translation(20,20).times(EditorMatrix(patch["affine"] ?? styles.first { EditorJSON.string($0["nodeId"])==id }?["affine"])).raw }
                }
                copies.append(copy)
            }
            if !copies.isEmpty { var variant=original;variant["nodePatches"]=EditorJSON.objects(original["nodePatches"])+copies;variant["approvalStatus"]="draft";commands.append(["op":"setVariant","variant":variant]) }
        }
        for node in source.reversed() where EditorJSON.bool(node["locked"]) { commands.append(["op":"setLocked","nodeId":ids[EditorJSON.string(node["id"])]!,"locked":true]) }
        return (["op":"batch","commands":commands],top.compactMap { ids[$0] })
    }
}

struct EditorPathSegment {
    var command: Character
    var points: [CGPoint]
}
struct EditorPath {
    var segments:[EditorPathSegment]
    enum Failure:LocalizedError { case syntax(String);var errorDescription:String? { if case .syntax(let message)=self { return message };return nil } }
    init(_ data:String) throws {
        guard data.utf8.count<=1024*1024 else { throw Failure.syntax("パスは 1 MiB 以内にしてください。") }
        let text=data as NSString,length=text.length
        let regex=try NSRegularExpression(pattern:"[+-]?(?:[0-9]+(?:\\.[0-9]*)?|\\.[0-9]+)(?:[eE][+-]?[0-9]+)?")
        var offset=0,command:Character?,current=CGPoint.zero,start=CGPoint.zero,cubic:CGPoint?,quad:CGPoint?,output:[EditorPathSegment]=[]
        func skip() { while offset<length && [9,10,13,32,44].contains(Int(text.character(at:offset))) { offset+=1 } }
        func number() throws -> Double {
            skip();guard let match=regex.firstMatch(in:data,options:.anchored,range:NSRange(location:offset,length:length-offset)),let value=Double(text.substring(with:match.range)),value.isFinite,abs(value)<=1_000_000 else { throw Failure.syntax("パス座標が不正です。±1,000,000 の有限値を指定してください。") }
            offset=match.range.upperBound;return value
        }
        while true {
            skip();if offset==length { break }
            let code=text.character(at:offset)
            if (65...90).contains(code)||(97...122).contains(code) { command=Character(UnicodeScalar(code)!);offset+=1 }
            guard let active=command else { throw Failure.syntax("パス命令が必要です。") }
            let relative=active.isLowercase,upper=Character(active.uppercased())
            func point() throws -> CGPoint { let x=try number(),y=try number();return CGPoint(x:x+(relative ? current.x:0),y:y+(relative ? current.y:0)) }
            func reflect(_ previous:CGPoint?) -> CGPoint { previous.map { CGPoint(x:2*current.x-$0.x,y:2*current.y-$0.y) } ?? current }
            var nextCubic:CGPoint?,nextQuad:CGPoint?,segment:EditorPathSegment
            switch upper {
            case "M": current=try point();start=current;segment=EditorPathSegment(command:"M",points:[current]);command=relative ? "l":"L"
            case "L": current=try point();segment=EditorPathSegment(command:"L",points:[current])
            case "H": let x=try number();current=CGPoint(x:x+(relative ? current.x:0),y:current.y);segment=EditorPathSegment(command:"L",points:[current])
            case "V": let y=try number();current=CGPoint(x:current.x,y:y+(relative ? current.y:0));segment=EditorPathSegment(command:"L",points:[current])
            case "C": let p1=try point(),p2=try point(),to=try point();segment=EditorPathSegment(command:"C",points:[p1,p2,to]);nextCubic=p2;current=to
            case "S": let p1=reflect(cubic),p2=try point(),to=try point();segment=EditorPathSegment(command:"C",points:[p1,p2,to]);nextCubic=p2;current=to
            case "Q": let control=try point(),to=try point();segment=EditorPathSegment(command:"Q",points:[control,to]);nextQuad=control;current=to
            case "T": let control=reflect(quad),to=try point();segment=EditorPathSegment(command:"Q",points:[control,to]);nextQuad=control;current=to
            case "Z": segment=EditorPathSegment(command:"Z",points:[]);current=start;command=nil
            default: throw Failure.syntax("未対応のパス命令です。元のデータは保持されます。")
            }
            if output.isEmpty && segment.command != "M" { throw Failure.syntax("パスは M から始めてください。") }
            guard segment.points.allSatisfy({$0.x.isFinite && $0.y.isFinite && abs($0.x)<=1_000_000 && abs($0.y)<=1_000_000}),output.count<200_000 else { throw Failure.syntax("パスの座標・点数が上限を超えています。") }
            output.append(segment);cubic=nextCubic;quad=nextQuad
        }
        guard !output.isEmpty else { throw Failure.syntax("パスの開始点が必要です。") }
        segments=output
    }
    private init(segments:[EditorPathSegment]) { self.segments=segments }
    func translated(_ dx:Double,_ dy:Double) -> EditorPath { EditorPath(segments:segments.map { EditorPathSegment(command:$0.command,points:$0.points.map { CGPoint(x:$0.x+dx,y:$0.y+dy) }) }) }
    func serialized() throws -> String {
        let value=segments.map { segment in String(segment.command)+segment.points.map { "\($0.x) \($0.y)" }.joined(separator:" ") }.joined(separator:" ")
        _=try EditorPath(value);return value
    }
}
