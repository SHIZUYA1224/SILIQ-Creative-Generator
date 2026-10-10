import AppKit
import Foundation

actor NativeTransport {
    private var handle: OpaquePointer?
    private let workspace: String
    private let fixtures: String
    init(workspace:String,fixtures:String) {self.workspace=workspace;self.fixtures=fixtures}
    private func decode(_ buffer:SiliqNativeBuffer) -> Data {
        defer {siliq_native_free(buffer)}
        guard let ptr=buffer.data,buffer.len>0 else{return Data()}
        return Data(bytes:ptr,count:buffer.len)
    }
    func request(_ data:Data) throws -> Data {
        if handle==nil {
            handle=workspace.withCString{workspace in fixtures.withCString{fixtures in siliq_native_create(workspace,fixtures)}}
            if handle==nil {return decode(siliq_native_last_error())}
        }
        return data.withUnsafeBytes{bytes in decode(siliq_native_request(handle,bytes.bindMemory(to:UInt8.self).baseAddress,data.count))}
    }
    deinit {if let handle{ siliq_native_close(handle) }}
}
@MainActor final class CoreClient {
    private let transport:NativeTransport
    init() {
        let support=FileManager.default.urls(for:.applicationSupportDirectory,in:.userDomainMask)[0]
        let workspace=support.appendingPathComponent("jp.siliq.creative-generator/workspace").path
        let fixtures=Bundle.main.resourceURL!.appendingPathComponent("public-safe").path
        transport=NativeTransport(workspace:workspace,fixtures:fixtures)
    }
    func call(_ command:String,_ args:[String:Any]=[:]) async throws -> Any {
        NSLog("SILIQ core request %@",command)
        let request=try JSONSerialization.data(withJSONObject:["command":command,"args":args],options:[.sortedKeys])
        let response=try await transport.request(request)
        let decoded=jsonObject(try JSONSerialization.jsonObject(with:response))
        NSLog("SILIQ core response %@ ok %d",command,jsonBool(decoded["ok"]) ? 1:0)
        guard jsonBool(decoded["ok"]) else {throw StudioFailure(issues:jsonObjects(decoded["issues"]).map(StudioIssue.init))}
        return decoded["value"] ?? NSNull()
    }
}
