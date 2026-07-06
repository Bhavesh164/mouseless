import Cocoa

for (i, screen) in NSScreen.screens.enumerated() {
    print("Screen \(i): frame=\(screen.frame), visibleFrame=\(screen.visibleFrame)")
}

let primaryHeight = NSScreen.screens.first?.frame.height ?? 0
print("Primary screen height: \(primaryHeight)")

let desktopBounds = NSScreen.screens.map(\.frame).reduce(CGRect.null) { $0.union($1) }
print("Desktop bounds: \(desktopBounds)")
