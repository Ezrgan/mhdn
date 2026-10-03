// Logs the on-screen window list once per second: z-order, layer, owner, bounds, frontmost app.
// Usage: swift tools/winlog.swift <seconds> > winlog.tsv
import AppKit
import CoreGraphics

let seconds = Int(CommandLine.arguments.dropFirst().first ?? "180") ?? 180
let start = Date()
while Date().timeIntervalSince(start) < Double(seconds) {
    let t = Int(Date().timeIntervalSince(start) * 1000)
    let front = NSWorkspace.shared.frontmostApplication?.localizedName ?? "?"
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    let list = (CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]]) ?? []
    var z = 0
    for info in list {
        let owner = info[kCGWindowOwnerName as String] as? String ?? "?"
        let layer = info[kCGWindowLayer as String] as? Int ?? -1
        let id = info[kCGWindowNumber as String] as? Int ?? 0
        let b = info[kCGWindowBounds as String] as? [String: Double] ?? [:]
        let alpha = info[kCGWindowAlpha as String] as? Double ?? 1
        z += 1
        if layer > 30 && owner != "mhdn" { continue }
        if (b["Width"] ?? 0) < 50 && owner != "mhdn" { continue }
        print("\(t)\tfront=\(front)\tz=\(z)\tid=\(id)\tlayer=\(layer)\towner=\(owner)\tbounds=\(Int(b["X"] ?? 0)),\(Int(b["Y"] ?? 0)),\(Int(b["Width"] ?? 0))x\(Int(b["Height"] ?? 0))\talpha=\(alpha)")
    }
    fflush(stdout)
    Thread.sleep(forTimeInterval: 1.0)
}
