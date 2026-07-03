import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

private let appName = "Mouseless"

struct Settings: Codable {
    var gridRows: Int = 5
    var gridColumns: Int = 5
    var overlayOpacity: Double = 0.72
    var continuousMode: Bool = false
    var freeModeStep: Double = 26
    var scrollStep: Int32 = 18
    var overlayHotkey: Hotkey = .defaultOverlay
    var quitGridKey: String = "Q"

    func clamped() -> Settings {
        var copy = self
        copy.gridRows = min(5, max(3, copy.gridRows))
        copy.gridColumns = min(5, max(3, copy.gridColumns))
        copy.overlayOpacity = min(0.95, max(0.25, copy.overlayOpacity))
        copy.freeModeStep = min(90, max(6, copy.freeModeStep))
        copy.overlayHotkey = copy.overlayHotkey.normalized()
        copy.quitGridKey = Hotkey.normalizedKey(copy.quitGridKey)
        if copy.quitGridKey.isEmpty {
            copy.quitGridKey = "Q"
        }
        return copy
    }
}

struct Hotkey: Codable {
    var key: String
    var command: Bool
    var option: Bool
    var control: Bool
    var shift: Bool

    static let defaultOverlay = Hotkey(key: "U", command: false, option: true, control: false, shift: false)

    var displayName: String {
        var parts: [String] = []
        if control { parts.append("Control") }
        if option { parts.append("Option") }
        if command { parts.append("Command") }
        if shift { parts.append("Shift") }
        parts.append(key)
        return parts.joined(separator: "+")
    }

    func matches(label: String, flags: CGEventFlags) -> Bool {
        normalizedKey(label) == normalizedKey(key)
            && flags.contains(.maskCommand) == command
            && flags.contains(.maskAlternate) == option
            && flags.contains(.maskControl) == control
            && flags.contains(.maskShift) == shift
    }

    func normalized() -> Hotkey {
        var copy = self
        copy.key = normalizedKey(copy.key)
        if !copy.command && !copy.option && !copy.control && !copy.shift {
            copy.option = true
        }
        return copy
    }

    static func fromInput(_ input: String, command: Bool, option: Bool, control: Bool, shift: Bool) -> Hotkey? {
        let normalized = normalizedKey(input)
        guard !normalized.isEmpty else { return nil }
        return Hotkey(key: normalized, command: command, option: option, control: control, shift: shift).normalized()
    }

    private func normalizedKey(_ value: String) -> String {
        Self.normalizedKey(value)
    }

    static func normalizedKey(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.count == 1 {
            return trimmed.uppercased()
        }

        switch trimmed.lowercased() {
        case "esc": return "Escape"
        case "return": return "Enter"
        case "left": return "ArrowLeft"
        case "right": return "ArrowRight"
        case "up": return "ArrowUp"
        case "down": return "ArrowDown"
        default:
            return trimmed.prefix(1).uppercased() + String(trimmed.dropFirst())
        }
    }
}

final class SettingsStore {
    private let url: URL
    var settings: Settings {
        didSet { save() }
    }

    init() {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let directory = support.appendingPathComponent(appName, isDirectory: true)
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        self.url = directory.appendingPathComponent("config.json")
        if let data = try? Data(contentsOf: url),
           let decoded = try? JSONDecoder().decode(Settings.self, from: data) {
            self.settings = decoded.clamped()
        } else {
            self.settings = Settings()
        }
    }

    private func save() {
        guard let data = try? JSONEncoder.pretty.encode(settings) else { return }
        try? data.write(to: url, options: .atomic)
    }
}

extension JSONEncoder {
    static var pretty: JSONEncoder {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        return encoder
    }
}

enum KeyLabel {
    static let byCode: [CGKeyCode: String] = [
        0: "A", 1: "S", 2: "D", 3: "F", 4: "H", 5: "G", 6: "Z", 7: "X",
        8: "C", 9: "V", 11: "B", 12: "Q", 13: "W", 14: "E", 15: "R",
        16: "Y", 17: "T", 18: "1", 19: "2", 20: "3", 21: "4", 22: "6",
        23: "5", 24: "=", 25: "9", 26: "7", 27: "-", 28: "8", 29: "0",
        30: "]", 31: "O", 32: "U", 33: "[", 34: "I", 35: "P", 36: "Enter",
        37: "L", 38: "J", 39: "'", 40: "K", 41: ";", 42: "\\", 43: ",",
        44: "/", 45: "N", 46: "M", 47: ".", 48: "Tab", 49: "Space",
        50: "`", 51: "Backspace", 53: "Escape", 123: "ArrowLeft",
        124: "ArrowRight", 125: "ArrowDown", 126: "ArrowUp"
    ]

    static let gridSequence = [
        "A", "S", "D", "F", "G",
        "H", "J", "K", "L", "Q",
        "W", "E", "R", "T", "Y",
        "U", "I", "O", "P", "Z",
        "X", "C", "V", "B", "N"
    ]
}

protocol EventTapDelegate: AnyObject {
    func handleKeyboardEvent(type: CGEventType, keyCode: CGKeyCode, label: String?, flags: CGEventFlags, isRepeat: Bool) -> Bool
}

final class EventTapManager {
    weak var delegate: EventTapDelegate?
    private var eventTap: CFMachPort?
    private var runLoopSource: CFRunLoopSource?

    func start() {
        let mask =
            (1 << CGEventType.keyDown.rawValue) |
            (1 << CGEventType.keyUp.rawValue) |
            (1 << CGEventType.flagsChanged.rawValue)

        let refcon = Unmanaged.passUnretained(self).toOpaque()
        guard let tap = CGEvent.tapCreate(
            tap: .cgSessionEventTap,
            place: .headInsertEventTap,
            options: .defaultTap,
            eventsOfInterest: CGEventMask(mask),
            callback: keyboardEventTapCallback,
            userInfo: refcon
        ) else {
            showPermissionAlert()
            return
        }

        eventTap = tap
        runLoopSource = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0)
        CFRunLoopAddSource(CFRunLoopGetCurrent(), runLoopSource, .commonModes)
        CGEvent.tapEnable(tap: tap, enable: true)
    }

    func stop() {
        if let tap = eventTap {
            CGEvent.tapEnable(tap: tap, enable: false)
        }
        if let source = runLoopSource {
            CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, .commonModes)
        }
        eventTap = nil
        runLoopSource = nil
    }

    fileprivate func handle(type: CGEventType, event: CGEvent) -> Bool {
        if type == .tapDisabledByTimeout || type == .tapDisabledByUserInput {
            if let tap = eventTap {
                CGEvent.tapEnable(tap: tap, enable: true)
            }
            return false
        }

        let code = CGKeyCode(event.getIntegerValueField(.keyboardEventKeycode))
        let label = KeyLabel.byCode[code]
        let isRepeat = event.getIntegerValueField(.keyboardEventAutorepeat) != 0
        return delegate?.handleKeyboardEvent(
            type: type,
            keyCode: code,
            label: label,
            flags: event.flags,
            isRepeat: isRepeat
        ) ?? false
    }

    private func showPermissionAlert() {
        let alert = NSAlert()
        alert.messageText = "\(appName) needs Accessibility permission"
        alert.informativeText = "Open System Settings > Privacy & Security > Accessibility and enable \(appName), then restart the app."
        alert.addButton(withTitle: "OK")
        alert.runModal()
    }
}

private func keyboardEventTapCallback(
    proxy: CGEventTapProxy,
    type: CGEventType,
    event: CGEvent,
    refcon: UnsafeMutableRawPointer?
) -> Unmanaged<CGEvent>? {
    guard let refcon else { return Unmanaged.passUnretained(event) }
    let manager = Unmanaged<EventTapManager>.fromOpaque(refcon).takeUnretainedValue()
    return manager.handle(type: type, event: event) ? nil : Unmanaged.passUnretained(event)
}

final class MouseController {
    private(set) var isDragging = false
    private var dragButton: CGMouseButton = .left

    func move(to appKitPoint: CGPoint) {
        let point = CoordinateSpace.appKitToQuartz(appKitPoint)
        CGWarpMouseCursorPosition(point)
        postMouse(type: .mouseMoved, at: appKitPoint, button: .left)
    }

    func click(at appKitPoint: CGPoint, button: CGMouseButton = .left, count: Int64 = 1) {
        move(to: appKitPoint)
        let down = mouseDownType(for: button)
        let up = mouseUpType(for: button)
        postMouse(type: down, at: appKitPoint, button: button, clickCount: count)
        postMouse(type: up, at: appKitPoint, button: button, clickCount: count)
    }

    func toggleDrag(at appKitPoint: CGPoint) {
        if isDragging {
            moveDrag(to: appKitPoint)
            postMouse(type: .leftMouseUp, at: appKitPoint, button: dragButton)
            isDragging = false
        } else {
            dragButton = .left
            move(to: appKitPoint)
            postMouse(type: .leftMouseDown, at: appKitPoint, button: dragButton)
            isDragging = true
        }
    }

    func moveDrag(to appKitPoint: CGPoint) {
        if isDragging {
            postMouse(type: .leftMouseDragged, at: appKitPoint, button: dragButton)
        } else {
            move(to: appKitPoint)
        }
    }

    func scroll(vertical: Int32, horizontal: Int32 = 0) {
        let source = CGEventSource(stateID: .hidSystemState)
        let event = CGEvent(
            scrollWheelEvent2Source: source,
            units: .pixel,
            wheelCount: 2,
            wheel1: vertical,
            wheel2: horizontal,
            wheel3: 0
        )
        event?.post(tap: .cghidEventTap)
    }

    private func postMouse(type: CGEventType, at appKitPoint: CGPoint, button: CGMouseButton, clickCount: Int64 = 1) {
        let source = CGEventSource(stateID: .hidSystemState)
        let event = CGEvent(
            mouseEventSource: source,
            mouseType: type,
            mouseCursorPosition: CoordinateSpace.appKitToQuartz(appKitPoint),
            mouseButton: button
        )
        event?.setIntegerValueField(.mouseEventClickState, value: clickCount)
        event?.post(tap: .cghidEventTap)
    }

    private func mouseDownType(for button: CGMouseButton) -> CGEventType {
        switch button {
        case .left: return .leftMouseDown
        case .right: return .rightMouseDown
        default: return .otherMouseDown
        }
    }

    private func mouseUpType(for button: CGMouseButton) -> CGEventType {
        switch button {
        case .left: return .leftMouseUp
        case .right: return .rightMouseUp
        default: return .otherMouseUp
        }
    }
}

enum CoordinateSpace {
    static var desktopBounds: CGRect {
        NSScreen.screens.map(\.frame).reduce(CGRect.null) { $0.union($1) }
    }

    static func appKitToQuartz(_ point: CGPoint) -> CGPoint {
        let bounds = desktopBounds
        return CGPoint(x: point.x, y: bounds.maxY - point.y)
    }

    static func quartzToAppKit(_ point: CGPoint) -> CGPoint {
        let bounds = desktopBounds
        return CGPoint(x: point.x, y: bounds.maxY - point.y)
    }
}

final class AccessibilityClickDetector {
    static let shared = AccessibilityClickDetector()
    private let clickableRoles: Set<String> = [
        "AXButton",
        "AXCheckBox",
        "AXRadioButton",
        "AXPopUpButton",
        "AXMenuButton",
        "AXMenuItem",
        "AXLink",
        "AXTextField",
        "AXTextArea",
        "AXComboBox",
        "AXSlider",
        "AXIncrementor",
        "AXDisclosureTriangle"
    ]
    private let pressAction = "AXPress"

    func hasClickableTarget(at appKitPoint: CGPoint) -> Bool {
        let quartzPoint = CoordinateSpace.appKitToQuartz(appKitPoint)
        if let element = element(at: quartzPoint), isClickable(element) {
            return true
        }
        if let element = element(at: appKitPoint), isClickable(element) {
            return true
        }
        return false
    }

    private func element(at point: CGPoint) -> AXUIElement? {
        let system = AXUIElementCreateSystemWide()
        var element: AXUIElement?
        let error = AXUIElementCopyElementAtPosition(system, Float(point.x), Float(point.y), &element)
        guard error == .success else { return nil }
        return element
    }

    private func isClickable(_ element: AXUIElement) -> Bool {
        var current: AXUIElement? = element
        for _ in 0..<6 {
            guard let candidate = current else { return false }
            if actionNames(for: candidate).contains(pressAction) {
                return true
            }
            if let role = stringAttribute("AXRole", from: candidate), clickableRoles.contains(role) {
                return true
            }
            current = parent(of: candidate)
        }
        return false
    }

    private func actionNames(for element: AXUIElement) -> [String] {
        var value: CFArray?
        guard AXUIElementCopyActionNames(element, &value) == .success,
              let actions = value as? [String]
        else { return [] }
        return actions
    }

    private func stringAttribute(_ attribute: String, from element: AXUIElement) -> String? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else {
            return nil
        }
        return value as? String
    }

    private func parent(of element: AXUIElement) -> AXUIElement? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, "AXParent" as CFString, &value) == .success else {
            return nil
        }
        return (value as! AXUIElement)
    }
}

final class OverlayController {
    private let settingsStore: SettingsStore
    private let mouse: MouseController
    private var windows: [OverlayWindow] = []
    private var history: [CGRect] = []
    private var activeRegion: CGRect = .zero
    private var targetOffset: CGSize = .zero
    private var currentScreenIndex = 0
    private var lastAction: (() -> Void)?
    private var actionStatus = "Enter click  Space force-click  Q quit  Tab persist"

    var isVisible: Bool {
        windows.contains { $0.isVisible }
    }

    init(settingsStore: SettingsStore, mouse: MouseController) {
        self.settingsStore = settingsStore
        self.mouse = mouse
    }

    func show() {
        rebuildWindows()
        let mousePoint = NSEvent.mouseLocation
        currentScreenIndex = NSScreen.screens.firstIndex { $0.frame.contains(mousePoint) } ?? 0
        activeRegion = NSScreen.screens[currentScreenIndex].frame
        targetOffset = .zero
        history.removeAll()
        actionStatus = "Enter click  Space force-click  \(settings.quitGridKey) quit  Tab persist"
        setWindowsVisible(true)
        redraw()
    }

    func hide() {
        setWindowsVisible(false)
        history.removeAll()
    }

    func toggle() {
        isVisible ? hide() : show()
    }

    func updateSettings() {
        redraw()
    }

    func handle(label: String) {
        if label == settings.quitGridKey || label == "Escape" {
            hide()
            return
        }

        switch label {
        case "Backspace":
            undo()
        case "Space", "1":
            perform("Left click") { self.mouse.click(at: self.virtualCursor) }
        case "2":
            perform("Double click") { self.mouse.click(at: self.virtualCursor, count: 2) }
        case "3":
            perform("Right click") { self.mouse.click(at: self.virtualCursor, button: .right) }
        case "4":
            perform(mouse.isDragging ? "Drop" : "Drag") { self.mouse.toggleDrag(at: self.virtualCursor) }
        case "Enter":
            clickIfAccessibleTarget()
        case "Tab":
            settingsStore.settings.continuousMode.toggle()
            actionStatus = settingsStore.settings.continuousMode ? "Persistent overlay on" : "Persistent overlay off"
            redraw()
        case "ArrowRight":
            isPrecisionMode ? nudgeTarget(dx: precisionNudgeStep, dy: 0) : moveScreen(delta: 1)
        case "ArrowLeft":
            isPrecisionMode ? nudgeTarget(dx: -precisionNudgeStep, dy: 0) : moveScreen(delta: -1)
        case "ArrowUp":
            if isPrecisionMode { nudgeTarget(dx: 0, dy: precisionNudgeStep) }
        case "ArrowDown":
            if isPrecisionMode { nudgeTarget(dx: 0, dy: -precisionNudgeStep) }
        case "=":
            settingsStore.settings.overlayOpacity = min(0.95, settingsStore.settings.overlayOpacity + 0.06)
            redraw()
        case "-":
            settingsStore.settings.overlayOpacity = max(0.25, settingsStore.settings.overlayOpacity - 0.06)
            redraw()
        case "`":
            lastAction?()
            if !settingsStore.settings.continuousMode { hide() }
        default:
            if let index = gridKeys.firstIndex(of: label) {
                selectCell(index: index)
            }
        }
    }

    private var settings: Settings {
        settingsStore.settings
    }

    private var gridKeys: [String] {
        Array(KeyLabel.gridSequence.prefix(settings.gridRows * settings.gridColumns))
    }

    private var virtualCursor: CGPoint {
        let x = min(activeRegion.maxX, max(activeRegion.minX, activeRegion.midX + targetOffset.width))
        let y = min(activeRegion.maxY, max(activeRegion.minY, activeRegion.midY + targetOffset.height))
        return CGPoint(x: x, y: y)
    }

    private var isPrecisionMode: Bool {
        let cellWidth = activeRegion.width / CGFloat(settings.gridColumns)
        let cellHeight = activeRegion.height / CGFloat(settings.gridRows)
        return min(cellWidth, cellHeight) < 24
    }

    private var precisionNudgeStep: CGFloat {
        max(1, min(8, min(activeRegion.width, activeRegion.height) / 18))
    }

    private func perform(_ status: String, action: @escaping () -> Void) {
        action()
        lastAction = action
        actionStatus = status
        redraw()
        if !settings.continuousMode && !mouse.isDragging {
            hide()
        }
    }

    private func clickIfAccessibleTarget() {
        guard AccessibilityClickDetector.shared.hasClickableTarget(at: virtualCursor) else {
            hide()
            return
        }
        perform("Clicked") { self.mouse.click(at: self.virtualCursor) }
    }

    private func selectCell(index: Int) {
        let row = index / settings.gridColumns
        let column = index % settings.gridColumns
        guard row < settings.gridRows else { return }

        history.append(activeRegion)
        let width = activeRegion.width / CGFloat(settings.gridColumns)
        let height = activeRegion.height / CGFloat(settings.gridRows)
        activeRegion = CGRect(
            x: activeRegion.minX + CGFloat(column) * width,
            y: activeRegion.maxY - CGFloat(row + 1) * height,
            width: width,
            height: height
        )
        targetOffset = .zero
        mouse.moveDrag(to: virtualCursor)
        actionStatus = isPrecisionMode
            ? "Precision \(labelForCurrentPath())  arrows nudge \(Int(precisionNudgeStep))px"
            : "Target \(labelForCurrentPath())"
        redraw()
    }

    private func undo() {
        guard let previous = history.popLast() else { return }
        activeRegion = previous
        targetOffset = .zero
        actionStatus = "Undo"
        redraw()
    }

    private func nudgeTarget(dx: CGFloat, dy: CGFloat) {
        let nextX = min(activeRegion.maxX, max(activeRegion.minX, virtualCursor.x + dx))
        let nextY = min(activeRegion.maxY, max(activeRegion.minY, virtualCursor.y + dy))
        targetOffset = CGSize(width: nextX - activeRegion.midX, height: nextY - activeRegion.midY)
        if mouse.isDragging {
            mouse.moveDrag(to: virtualCursor)
        }
        actionStatus = "Nudged \(Int(precisionNudgeStep))px  Enter click  Space force-click"
        redraw()
    }

    private func moveScreen(delta: Int) {
        let screens = NSScreen.screens
        guard !screens.isEmpty else { return }
        currentScreenIndex = (currentScreenIndex + delta + screens.count) % screens.count
        activeRegion = screens[currentScreenIndex].frame
        targetOffset = .zero
        history.removeAll()
        actionStatus = "Monitor \(currentScreenIndex + 1)"
        redraw()
    }

    private func labelForCurrentPath() -> String {
        let size = Int(max(1, min(activeRegion.width, activeRegion.height)).rounded())
        return "\(size)px cell"
    }

    private func rebuildWindows() {
        windows.forEach { $0.close() }
        windows = NSScreen.screens.map { screen in
            let window = OverlayWindow(screen: screen)
            window.overlayView.dataSource = self
            return window
        }
    }

    private func setWindowsVisible(_ visible: Bool) {
        for window in windows {
            if visible {
                window.orderFrontRegardless()
            } else {
                window.orderOut(nil)
            }
        }
    }

    private func redraw() {
        windows.forEach { $0.overlayView.needsDisplay = true }
    }
}

extension OverlayController: OverlayViewDataSource {
    func snapshot(for view: OverlayView) -> OverlaySnapshot {
        OverlaySnapshot(
            screenFrame: view.screenFrame,
            activeRegion: activeRegion,
            rows: settings.gridRows,
            columns: settings.gridColumns,
            labels: gridKeys,
            opacity: settings.overlayOpacity,
            cursor: virtualCursor,
            status: actionStatus,
            continuousMode: settings.continuousMode,
            dragging: mouse.isDragging,
            precisionMode: isPrecisionMode
        )
    }
}

struct OverlaySnapshot {
    let screenFrame: CGRect
    let activeRegion: CGRect
    let rows: Int
    let columns: Int
    let labels: [String]
    let opacity: Double
    let cursor: CGPoint
    let status: String
    let continuousMode: Bool
    let dragging: Bool
    let precisionMode: Bool
}

protocol OverlayViewDataSource: AnyObject {
    func snapshot(for view: OverlayView) -> OverlaySnapshot
}

final class OverlayWindow: NSPanel {
    let overlayView: OverlayView

    init(screen: NSScreen) {
        self.overlayView = OverlayView(screenFrame: screen.frame)
        super.init(
            contentRect: screen.frame,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        self.level = .screenSaver
        self.backgroundColor = .clear
        self.isOpaque = false
        self.hasShadow = false
        self.ignoresMouseEvents = true
        self.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
        self.contentView = overlayView
        self.setAccessibilityElement(false)
        self.setAccessibilityHidden(true)
        overlayView.setAccessibilityElement(false)
        overlayView.setAccessibilityHidden(true)
    }
}

final class OverlayView: NSView {
    weak var dataSource: OverlayViewDataSource?
    let screenFrame: CGRect

    override var isFlipped: Bool { true }

    init(screenFrame: CGRect) {
        self.screenFrame = screenFrame
        super.init(frame: CGRect(origin: .zero, size: screenFrame.size))
        wantsLayer = true
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func draw(_ dirtyRect: NSRect) {
        guard let snapshot = dataSource?.snapshot(for: self),
              bounds.intersects(bounds)
        else { return }

        let context = NSGraphicsContext.current?.cgContext
        context?.setShouldAntialias(true)

        NSColor.black.withAlphaComponent(snapshot.opacity * 0.48).setFill()
        bounds.fill()

        guard snapshot.screenFrame.intersects(snapshot.activeRegion) else {
            drawStatus(snapshot)
            return
        }

        let localRegion = localRect(for: snapshot.activeRegion)
        let gridRegion = snapshot.precisionMode ? precisionGridRect(around: localRegion, snapshot: snapshot) : localRegion
        NSColor.black.withAlphaComponent(snapshot.opacity * 0.16).setFill()
        localRegion.fill()
        if snapshot.precisionMode {
            drawPrecisionAnchor(from: localRegion, to: gridRegion)
        }
        drawGrid(snapshot, in: gridRegion)
        drawCursor(snapshot)
        if snapshot.precisionMode {
            drawPrecisionCursor(snapshot, in: gridRegion)
        }
        drawStatus(snapshot)
    }

    private func precisionGridRect(around localRegion: CGRect, snapshot: OverlaySnapshot) -> CGRect {
        let width = min(bounds.width - 48, max(320, CGFloat(snapshot.columns) * 64))
        let height = min(bounds.height - 96, max(240, CGFloat(snapshot.rows) * 52))
        var origin = CGPoint(
            x: localRegion.midX - width / 2,
            y: localRegion.midY - height / 2
        )
        origin.x = min(bounds.maxX - width - 24, max(bounds.minX + 24, origin.x))
        origin.y = min(bounds.maxY - height - 58, max(bounds.minY + 24, origin.y))
        return CGRect(origin: origin, size: CGSize(width: width, height: height))
    }

    private func drawPrecisionAnchor(from localRegion: CGRect, to gridRegion: CGRect) {
        NSColor.systemYellow.withAlphaComponent(0.90).setStroke()
        let outline = NSBezierPath(rect: localRegion)
        outline.lineWidth = 2
        outline.stroke()

        NSColor.black.withAlphaComponent(0.40).setFill()
        NSBezierPath(roundedRect: gridRegion.insetBy(dx: -8, dy: -8), xRadius: 8, yRadius: 8).fill()

        NSColor.systemYellow.withAlphaComponent(0.60).setStroke()
        let connector = NSBezierPath()
        connector.move(to: CGPoint(x: localRegion.midX, y: localRegion.midY))
        connector.line(to: CGPoint(x: gridRegion.midX, y: gridRegion.midY))
        connector.lineWidth = 1
        connector.stroke()
    }

    private func drawGrid(_ snapshot: OverlaySnapshot, in rect: CGRect) {
        let lineColor = NSColor.systemTeal.withAlphaComponent(0.82)
        let softColor = NSColor.systemTeal.withAlphaComponent(0.24)
        let textColor = NSColor.white
        let width = rect.width / CGFloat(snapshot.columns)
        let height = rect.height / CGFloat(snapshot.rows)

        for row in 0..<snapshot.rows {
            for column in 0..<snapshot.columns {
                let index = row * snapshot.columns + column
                guard index < snapshot.labels.count else { continue }
                let cell = CGRect(
                    x: rect.minX + CGFloat(column) * width,
                    y: rect.minY + CGFloat(row) * height,
                    width: width,
                    height: height
                )

                let path = NSBezierPath(rect: cell)
                softColor.setFill()
                path.fill()
                lineColor.setStroke()
                path.lineWidth = 1
                path.stroke()

                drawLabel(snapshot.labels[index], in: cell, color: textColor)
            }
        }
    }

    private func drawLabel(_ label: String, in cell: CGRect, color: NSColor) {
        let shortestSide = min(cell.width, cell.height)
        guard shortestSide >= 9 else { return }

        let targetSize = min(42, max(8, shortestSide * 0.48))
        var fontSize = targetSize
        var attributed: NSAttributedString
        var labelSize: CGSize

        repeat {
            let attributes: [NSAttributedString.Key: Any] = [
                .font: NSFont.monospacedSystemFont(ofSize: fontSize, weight: .bold),
                .foregroundColor: color
            ]
            attributed = NSAttributedString(string: label, attributes: attributes)
            labelSize = attributed.size()
            if labelSize.width <= cell.width * 0.72 && labelSize.height <= cell.height * 0.72 {
                break
            }
            fontSize -= 1
        } while fontSize >= 7

        let paddingX = max(2, fontSize * 0.28)
        let paddingY = max(1, fontSize * 0.12)
        let background = CGRect(
            x: cell.midX - labelSize.width / 2 - paddingX,
            y: cell.midY - labelSize.height / 2 - paddingY,
            width: labelSize.width + paddingX * 2,
            height: labelSize.height + paddingY * 2
        )
        NSColor.black.withAlphaComponent(0.30).setFill()
        NSBezierPath(roundedRect: background, xRadius: min(4, paddingX), yRadius: min(4, paddingY + 1)).fill()
        attributed.draw(at: CGPoint(
            x: cell.midX - labelSize.width / 2,
            y: cell.midY - labelSize.height / 2
        ))
    }

    private func drawCursor(_ snapshot: OverlaySnapshot) {
        guard screenFrame.contains(snapshot.cursor) else { return }
        let local = localPoint(for: snapshot.cursor)
        let radius: CGFloat = snapshot.dragging ? 9 : 7
        let color = snapshot.dragging ? NSColor.systemOrange : NSColor.systemGreen
        let rect = CGRect(x: local.x - radius, y: local.y - radius, width: radius * 2, height: radius * 2)
        color.setFill()
        NSBezierPath(ovalIn: rect).fill()
        NSColor.white.withAlphaComponent(0.9).setStroke()
        let ring = NSBezierPath(ovalIn: rect.insetBy(dx: -4, dy: -4))
        ring.lineWidth = 2
        ring.stroke()
    }

    private func drawPrecisionCursor(_ snapshot: OverlaySnapshot, in gridRegion: CGRect) {
        guard snapshot.activeRegion.width > 0, snapshot.activeRegion.height > 0 else { return }
        let normalizedX = (snapshot.cursor.x - snapshot.activeRegion.minX) / snapshot.activeRegion.width
        let normalizedY = (snapshot.activeRegion.maxY - snapshot.cursor.y) / snapshot.activeRegion.height
        let point = CGPoint(
            x: gridRegion.minX + normalizedX * gridRegion.width,
            y: gridRegion.minY + normalizedY * gridRegion.height
        )
        let radius: CGFloat = 8
        NSColor.systemPink.setFill()
        NSBezierPath(ovalIn: CGRect(x: point.x - radius, y: point.y - radius, width: radius * 2, height: radius * 2)).fill()
        NSColor.white.withAlphaComponent(0.95).setStroke()
        let crosshair = NSBezierPath()
        crosshair.move(to: CGPoint(x: point.x - 16, y: point.y))
        crosshair.line(to: CGPoint(x: point.x + 16, y: point.y))
        crosshair.move(to: CGPoint(x: point.x, y: point.y - 16))
        crosshair.line(to: CGPoint(x: point.x, y: point.y + 16))
        crosshair.lineWidth = 2
        crosshair.stroke()
    }

    private func drawStatus(_ snapshot: OverlaySnapshot) {
        let mode = snapshot.continuousMode ? "Persist" : "Once"
        let drag = snapshot.dragging ? "  Dragging" : ""
        let text = "\(appName)  \(mode)\(drag)  \(snapshot.status)"
        let attributes: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 14, weight: .semibold),
            .foregroundColor: NSColor.white,
            .backgroundColor: NSColor.black.withAlphaComponent(0.52)
        ]
        let attributed = NSAttributedString(string: " \(text) ", attributes: attributes)
        attributed.draw(at: CGPoint(x: 18, y: bounds.height - 34))
    }

    private func localRect(for appKitRect: CGRect) -> CGRect {
        CGRect(
            x: appKitRect.minX - screenFrame.minX,
            y: screenFrame.maxY - appKitRect.maxY,
            width: appKitRect.width,
            height: appKitRect.height
        )
    }

    private func localPoint(for appKitPoint: CGPoint) -> CGPoint {
        CGPoint(
            x: appKitPoint.x - screenFrame.minX,
            y: screenFrame.maxY - appKitPoint.y
        )
    }
}

final class FreeModeController {
    private let settingsStore: SettingsStore
    private let mouse: MouseController
    private var active = false

    var isActive: Bool { active }

    init(settingsStore: SettingsStore, mouse: MouseController) {
        self.settingsStore = settingsStore
        self.mouse = mouse
    }

    func toggle() {
        active ? exit() : enter()
    }

    func enter() {
        active = true
        ToastWindow.shared.show("Free mode")
    }

    func exit() {
        active = false
        ToastWindow.shared.show("Free mode off")
    }

    func handle(label: String) -> Bool {
        guard active else { return false }
        let step = CGFloat(settingsStore.settings.freeModeStep)
        var point = NSEvent.mouseLocation

        switch label {
        case "Escape":
            exit()
        case "H", "ArrowLeft":
            point.x -= step
            mouse.move(to: point)
        case "L", "ArrowRight":
            point.x += step
            mouse.move(to: point)
        case "K", "ArrowUp":
            point.y += step
            mouse.move(to: point)
        case "J", "ArrowDown":
            point.y -= step
            mouse.move(to: point)
        case "Space":
            mouse.click(at: point)
        case "R":
            mouse.click(at: point, button: .right)
        case "U":
            mouse.scroll(vertical: settingsStore.settings.scrollStep)
        case "D":
            mouse.scroll(vertical: -settingsStore.settings.scrollStep)
        case "Y":
            mouse.scroll(vertical: 0, horizontal: -settingsStore.settings.scrollStep)
        case "O":
            mouse.scroll(vertical: 0, horizontal: settingsStore.settings.scrollStep)
        default:
            break
        }
        return true
    }
}

final class ToastWindow: NSPanel {
    static let shared = ToastWindow()
    private let label = NSTextField(labelWithString: "")
    private var hideWorkItem: DispatchWorkItem?

    private init() {
        let frame = CGRect(x: 0, y: 0, width: 220, height: 52)
        super.init(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        level = .floating
        backgroundColor = .clear
        isOpaque = false
        hasShadow = true
        ignoresMouseEvents = true
        collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]

        let visual = NSVisualEffectView(frame: frame)
        visual.material = .hudWindow
        visual.blendingMode = .behindWindow
        visual.state = .active
        visual.wantsLayer = true
        visual.layer?.cornerRadius = 8

        label.frame = frame.insetBy(dx: 12, dy: 14)
        label.alignment = .center
        label.font = .systemFont(ofSize: 15, weight: .semibold)
        label.textColor = .white
        visual.addSubview(label)
        contentView = visual
    }

    func show(_ text: String) {
        label.stringValue = text
        if let screen = NSScreen.main {
            setFrameOrigin(CGPoint(x: screen.frame.midX - frame.width / 2, y: screen.frame.maxY - 110))
        }
        orderFrontRegardless()
        hideWorkItem?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.orderOut(nil) }
        hideWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2, execute: work)
    }
}

final class PreferencesWindowController: NSWindowController {
    private let settingsStore: SettingsStore
    private let onChange: () -> Void

    private let rowsSlider = NSSlider(value: 5, minValue: 3, maxValue: 5, target: nil, action: nil)
    private let columnsSlider = NSSlider(value: 5, minValue: 3, maxValue: 5, target: nil, action: nil)
    private let opacitySlider = NSSlider(value: 0.72, minValue: 0.25, maxValue: 0.95, target: nil, action: nil)
    private let speedSlider = NSSlider(value: 26, minValue: 6, maxValue: 90, target: nil, action: nil)
    private let continuousCheckbox = NSButton(checkboxWithTitle: "Keep overlay visible after actions", target: nil, action: nil)
    private let shortcutKeyField = NSTextField(string: "U")
    private let shortcutCommandCheckbox = NSButton(checkboxWithTitle: "Command", target: nil, action: nil)
    private let shortcutOptionCheckbox = NSButton(checkboxWithTitle: "Option", target: nil, action: nil)
    private let shortcutControlCheckbox = NSButton(checkboxWithTitle: "Control", target: nil, action: nil)
    private let shortcutShiftCheckbox = NSButton(checkboxWithTitle: "Shift", target: nil, action: nil)
    private let shortcutPreview = NSTextField(labelWithString: "")
    private let quitGridKeyField = NSTextField(string: "Q")
    private let quitGridKeyPreview = NSTextField(labelWithString: "")

    init(settingsStore: SettingsStore, onChange: @escaping () -> Void) {
        self.settingsStore = settingsStore
        self.onChange = onChange
        let window = NSWindow(
            contentRect: CGRect(x: 0, y: 0, width: 580, height: 380),
            styleMask: [.titled, .closable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.title = "\(appName) Preferences"
        super.init(window: window)
        buildUI()
        syncFromSettings()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    func show() {
        syncFromSettings()
        window?.center()
        window?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func buildUI() {
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.spacing = 14
        stack.edgeInsets = NSEdgeInsets(top: 22, left: 22, bottom: 18, right: 22)
        stack.translatesAutoresizingMaskIntoConstraints = false

        rowsSlider.target = self
        rowsSlider.action = #selector(updateRows)
        columnsSlider.target = self
        columnsSlider.action = #selector(updateColumns)
        opacitySlider.target = self
        opacitySlider.action = #selector(updateOpacity)
        speedSlider.target = self
        speedSlider.action = #selector(updateSpeed)
        continuousCheckbox.target = self
        continuousCheckbox.action = #selector(updateContinuous)
        shortcutKeyField.target = self
        shortcutKeyField.action = #selector(updateShortcut)
        shortcutCommandCheckbox.target = self
        shortcutCommandCheckbox.action = #selector(updateShortcut)
        shortcutOptionCheckbox.target = self
        shortcutOptionCheckbox.action = #selector(updateShortcut)
        shortcutControlCheckbox.target = self
        shortcutControlCheckbox.action = #selector(updateShortcut)
        shortcutShiftCheckbox.target = self
        shortcutShiftCheckbox.action = #selector(updateShortcut)
        quitGridKeyField.target = self
        quitGridKeyField.action = #selector(updateQuitGridKey)

        stack.addArrangedSubview(row("Rows", rowsSlider))
        stack.addArrangedSubview(row("Columns", columnsSlider))
        stack.addArrangedSubview(row("Overlay opacity", opacitySlider))
        stack.addArrangedSubview(row("Free-mode step", speedSlider))
        stack.addArrangedSubview(shortcutRow())
        stack.addArrangedSubview(quitGridKeyRow())
        stack.addArrangedSubview(continuousCheckbox)

        let hint = NSTextField(labelWithString: "Overlay shortcut defaults to Option+U. Free mode is available from the status-bar menu.")
        hint.font = .systemFont(ofSize: 12)
        hint.textColor = .secondaryLabelColor
        stack.addArrangedSubview(hint)

        window?.contentView = NSView()
        window?.contentView?.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: window!.contentView!.leadingAnchor),
            stack.trailingAnchor.constraint(equalTo: window!.contentView!.trailingAnchor),
            stack.topAnchor.constraint(equalTo: window!.contentView!.topAnchor),
            stack.bottomAnchor.constraint(equalTo: window!.contentView!.bottomAnchor)
        ])
    }

    private func row(_ title: String, _ slider: NSSlider) -> NSView {
        let view = NSStackView()
        view.orientation = .horizontal
        view.spacing = 12
        let label = NSTextField(labelWithString: title)
        label.widthAnchor.constraint(equalToConstant: 130).isActive = true
        slider.widthAnchor.constraint(equalToConstant: 260).isActive = true
        view.addArrangedSubview(label)
        view.addArrangedSubview(slider)
        return view
    }

    private func shortcutRow() -> NSView {
        let view = NSStackView()
        view.orientation = .horizontal
        view.spacing = 12

        let label = NSTextField(labelWithString: "Overlay shortcut")
        label.widthAnchor.constraint(equalToConstant: 130).isActive = true

        let controls = NSStackView()
        controls.orientation = .vertical
        controls.spacing = 8

        let modifierRow = NSStackView()
        modifierRow.orientation = .horizontal
        modifierRow.spacing = 10
        modifierRow.addArrangedSubview(shortcutOptionCheckbox)
        modifierRow.addArrangedSubview(shortcutCommandCheckbox)
        modifierRow.addArrangedSubview(shortcutControlCheckbox)
        modifierRow.addArrangedSubview(shortcutShiftCheckbox)

        let keyRow = NSStackView()
        keyRow.orientation = .horizontal
        keyRow.spacing = 8
        let keyLabel = NSTextField(labelWithString: "Key")
        shortcutKeyField.widthAnchor.constraint(equalToConstant: 70).isActive = true
        shortcutPreview.textColor = .secondaryLabelColor
        keyRow.addArrangedSubview(keyLabel)
        keyRow.addArrangedSubview(shortcutKeyField)
        keyRow.addArrangedSubview(shortcutPreview)

        controls.addArrangedSubview(modifierRow)
        controls.addArrangedSubview(keyRow)
        view.addArrangedSubview(label)
        view.addArrangedSubview(controls)
        return view
    }

    private func quitGridKeyRow() -> NSView {
        let view = NSStackView()
        view.orientation = .horizontal
        view.spacing = 12

        let label = NSTextField(labelWithString: "Quit grid key")
        label.widthAnchor.constraint(equalToConstant: 130).isActive = true

        let keyLabel = NSTextField(labelWithString: "Key")
        quitGridKeyField.widthAnchor.constraint(equalToConstant: 70).isActive = true
        quitGridKeyPreview.textColor = .secondaryLabelColor

        view.addArrangedSubview(label)
        view.addArrangedSubview(keyLabel)
        view.addArrangedSubview(quitGridKeyField)
        view.addArrangedSubview(quitGridKeyPreview)
        return view
    }

    private func syncFromSettings() {
        let settings = settingsStore.settings
        rowsSlider.integerValue = settings.gridRows
        columnsSlider.integerValue = settings.gridColumns
        opacitySlider.doubleValue = settings.overlayOpacity
        speedSlider.doubleValue = settings.freeModeStep
        continuousCheckbox.state = settings.continuousMode ? .on : .off
        shortcutKeyField.stringValue = settings.overlayHotkey.key
        shortcutCommandCheckbox.state = settings.overlayHotkey.command ? .on : .off
        shortcutOptionCheckbox.state = settings.overlayHotkey.option ? .on : .off
        shortcutControlCheckbox.state = settings.overlayHotkey.control ? .on : .off
        shortcutShiftCheckbox.state = settings.overlayHotkey.shift ? .on : .off
        shortcutPreview.stringValue = settings.overlayHotkey.displayName
        quitGridKeyField.stringValue = settings.quitGridKey
        quitGridKeyPreview.stringValue = "Hides the grid"
    }

    @objc private func updateRows() {
        settingsStore.settings.gridRows = rowsSlider.integerValue
        onChange()
    }

    @objc private func updateColumns() {
        settingsStore.settings.gridColumns = columnsSlider.integerValue
        onChange()
    }

    @objc private func updateOpacity() {
        settingsStore.settings.overlayOpacity = opacitySlider.doubleValue
        onChange()
    }

    @objc private func updateSpeed() {
        settingsStore.settings.freeModeStep = speedSlider.doubleValue
        onChange()
    }

    @objc private func updateContinuous() {
        settingsStore.settings.continuousMode = continuousCheckbox.state == .on
        onChange()
    }

    @objc private func updateShortcut() {
        guard let hotkey = Hotkey.fromInput(
            shortcutKeyField.stringValue,
            command: shortcutCommandCheckbox.state == .on,
            option: shortcutOptionCheckbox.state == .on,
            control: shortcutControlCheckbox.state == .on,
            shift: shortcutShiftCheckbox.state == .on
        ) else {
            shortcutPreview.stringValue = "Enter a key"
            return
        }

        settingsStore.settings.overlayHotkey = hotkey
        syncFromSettings()
        onChange()
    }

    @objc private func updateQuitGridKey() {
        let key = Hotkey.normalizedKey(quitGridKeyField.stringValue)
        guard !key.isEmpty else {
            quitGridKeyPreview.stringValue = "Enter a key"
            return
        }

        settingsStore.settings.quitGridKey = key
        syncFromSettings()
        onChange()
    }
}

final class StatusBarController {
    private let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)

    init(showOverlay: @escaping () -> Void, toggleFreeMode: @escaping () -> Void, preferences: @escaping () -> Void) {
        item.button?.title = "Mouseless"
        item.button?.toolTip = appName

        let menu = NSMenu()
        menu.addCallbackItem(title: "Show Overlay", action: showOverlay)
        menu.addCallbackItem(title: "Toggle Free Mode", action: toggleFreeMode)
        menu.addItem(.separator())
        menu.addCallbackItem(title: "Preferences...", action: preferences)
        menu.addItem(.separator())
        menu.addCallbackItem(title: "Quit \(appName)") { NSApp.terminate(nil) }
        item.menu = menu
    }
}

private final class CallbackMenuItem: NSMenuItem {
    let callback: () -> Void

    init(title: String, callback: @escaping () -> Void) {
        self.callback = callback
        super.init(title: title, action: #selector(runCallback), keyEquivalent: "")
        target = self
    }

    required init(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    @objc private func runCallback() {
        callback()
    }
}

extension NSMenu {
    func addCallbackItem(title: String, action: @escaping () -> Void) {
        addItem(CallbackMenuItem(title: title, callback: action))
    }
}

final class AppDelegate: NSObject, NSApplicationDelegate, EventTapDelegate {
    private let settingsStore = SettingsStore()
    private let eventTap = EventTapManager()
    private let mouse = MouseController()
    private lazy var overlay = OverlayController(settingsStore: settingsStore, mouse: mouse)
    private lazy var freeMode = FreeModeController(settingsStore: settingsStore, mouse: mouse)
    private var statusBar: StatusBarController?
    private var preferences: PreferencesWindowController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
        requestAccessibilityPermission()

        eventTap.delegate = self
        eventTap.start()

        preferences = PreferencesWindowController(settingsStore: settingsStore) { [weak self] in
            self?.overlay.updateSettings()
        }

        statusBar = StatusBarController(
            showOverlay: { [weak self] in self?.overlay.show() },
            toggleFreeMode: { [weak self] in self?.freeMode.toggle() },
            preferences: { [weak self] in self?.preferences?.show() }
        )

        ToastWindow.shared.show("\(appName) ready")
    }

    func applicationWillTerminate(_ notification: Notification) {
        eventTap.stop()
    }

    func handleKeyboardEvent(type: CGEventType, keyCode: CGKeyCode, label: String?, flags: CGEventFlags, isRepeat: Bool) -> Bool {
        guard type == .keyDown || type == .keyUp else { return false }
        guard let label else { return false }

        if type == .keyUp {
            return overlay.isVisible || freeMode.isActive
        }

        if settingsStore.settings.overlayHotkey.matches(label: label, flags: flags) {
            overlay.toggle()
            return true
        }

        if overlay.isVisible {
            overlay.handle(label: label)
            return true
        }

        if freeMode.isActive {
            return freeMode.handle(label: label)
        }

        return false
    }

    private func requestAccessibilityPermission() {
        let key = "AXTrustedCheckOptionPrompt"
        let options = [key: true] as CFDictionary
        _ = AXIsProcessTrustedWithOptions(options)
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.run()
