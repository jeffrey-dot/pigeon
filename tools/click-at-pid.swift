// Posts a click inside the first on-screen window of a process, without needing it to be
// frontmost. `tools/m2-gui-check.sh` uses this: this session cannot activate windows, but
// pid-targeted events still reach the view under the point.
//
// usage: swift tools/click-at-pid.swift <pid> <dx> <dy>
import CoreGraphics
import Foundation

let args = CommandLine.arguments
guard args.count == 4, let pid = Int32(args[1]), let dx = Double(args[2]), let dy = Double(args[3])
else {
    FileHandle.standardError.write(Data("usage: click-at-pid <pid> <dx> <dy>\n".utf8))
    exit(2)
}
let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
    as? [[String: Any]] ?? []
guard let window = windows.first(where: { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }),
      let bounds = window[kCGWindowBounds as String] as? [String: Double],
      let x = bounds["X"], let y = bounds["Y"]
else {
    FileHandle.standardError.write(Data("click-at-pid: no on-screen window for pid \(pid)\n".utf8))
    exit(3)
}
let point = CGPoint(x: x + dx, y: y + dy)
// A background click needs the window under the pointer, or the app drops the event.
let windowNumber = (window[kCGWindowNumber as String] as? Int) ?? 0
for type in [CGEventType.leftMouseDown, .leftMouseUp] {
    guard let event = CGEvent(
        mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: .left)
    else { exit(4) }
    event.setIntegerValueField(.mouseEventWindowUnderMousePointer, value: Int64(windowNumber))
    event.setIntegerValueField(
        .mouseEventWindowUnderMousePointerThatCanHandleThisEvent, value: Int64(windowNumber))
    event.postToPid(pid)
    usleep(80_000)
}
print("clicked \(Int(point.x)),\(Int(point.y))")
