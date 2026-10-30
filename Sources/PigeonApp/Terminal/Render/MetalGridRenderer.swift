import Foundation
import Metal
import QuartzCore

/// Draws one pane's grid on the GPU from its own thread (issue #1).
///
/// The main thread only posts `GridFrame` values into a `FrameMailbox`; this thread takes
/// the newest one, rebuilds the instance data of the dirty rows, and encodes one instanced
/// draw per pass (backgrounds, glyphs, decorations + cursor).
///
/// Frames are paced by a `CAMetalDisplayLink` on the thread's run loop (macOS 14+): a post
/// unpauses the link, the next refresh draws whatever is newest, and an empty mailbox pauses
/// it again. A burst of output (`cat` of a large file) costs one frame per refresh, not one
/// per delta, and an idle pane costs no CPU or GPU time. macOS throttles the link of an
/// occluded window to about 1 Hz, so a pane behind other windows stops rendering too. macOS 13 has no display link for
/// Metal layers, so there a post draws on the next run-loop turn instead.
///
/// Instance buffers rotate through `inFlight` slots guarded by a semaphore (triple
/// buffering): the CPU fills frame N+1 while the GPU still reads frame N.
final class MetalGridRenderer: @unchecked Sendable {
    static let inFlight = 3

    let layer: CAMetalLayer
    private let device: MTLDevice
    private let queue: MTLCommandQueue
    private let rectPipeline: MTLRenderPipelineState
    private let glyphPipeline: MTLRenderPipelineState
    private let mailbox = FrameMailbox()
    private let slots = DispatchSemaphore(value: MetalGridRenderer.inFlight)
    private var thread: Thread?
    /// The render thread's run loop, published once by `start()`.
    private var runLoop: CFRunLoop?
    /// `CAMetalDisplayLink` on macOS 14+; typed `AnyObject` so the class needs no availability.
    private var link: AnyObject?
    private var linkDelegate: AnyObject?

    // Render-thread state.
    private var glyphs: GlyphCache?
    private var instances = GridInstances()
    private var buffers: [[MTLBuffer?]] = Array(
        repeating: [nil, nil, nil], count: MetalGridRenderer.inFlight)
    private var slot = 0

    private static let debug = ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil
    /// Render-thread only; GPU samples are handed back to this thread by `perform`.
    private var stats = FrameStats(renderer: "metal")

    /// Nil when there is no Metal device or the shaders do not compile; the pane then keeps
    /// the CoreText path.
    init?() {
        guard let device = MTLCreateSystemDefaultDevice(),
              let queue = device.makeCommandQueue(),
              let library = try? device.makeLibrary(source: MetalShaders.source, options: nil),
              let rect = Self.pipeline(device, library, vertex: "rect_vertex", fragment: "rect_fragment"),
              let glyph = Self.pipeline(device, library, vertex: "glyph_vertex", fragment: "glyph_fragment")
        else { return nil }
        self.device = device
        self.queue = queue
        rectPipeline = rect
        glyphPipeline = glyph
        layer = CAMetalLayer()
        layer.device = device
        layer.pixelFormat = .bgra8Unorm
        layer.framebufferOnly = true
        layer.isOpaque = true
        // Frames only exist when the screen changed; let them reach the display as soon as
        // vsync allows instead of queueing behind a timer.
        layer.maximumDrawableCount = 3
        layer.displaySyncEnabled = true
        layer.allowsNextDrawableTimeout = true
        // The layer's colour space must match how the palette was resolved (sRGB bytes).
        layer.colorspace = CGColorSpace(name: CGColorSpace.sRGB)
    }

    private static func pipeline(
        _ device: MTLDevice, _ library: MTLLibrary, vertex: String, fragment: String
    ) -> MTLRenderPipelineState? {
        let descriptor = MTLRenderPipelineDescriptor()
        descriptor.vertexFunction = library.makeFunction(name: vertex)
        descriptor.fragmentFunction = library.makeFunction(name: fragment)
        guard let attachment = descriptor.colorAttachments[0] else { return nil }
        attachment.pixelFormat = .bgra8Unorm
        // Premultiplied alpha "over".
        attachment.isBlendingEnabled = true
        attachment.sourceRGBBlendFactor = .one
        attachment.sourceAlphaBlendFactor = .one
        attachment.destinationRGBBlendFactor = .oneMinusSourceAlpha
        attachment.destinationAlphaBlendFactor = .oneMinusSourceAlpha
        return try? device.makeRenderPipelineState(descriptor: descriptor)
    }

    func start() {
        guard thread == nil else { return }
        let ready = DispatchSemaphore(value: 0)
        let thread = Thread { [weak self] in
            guard let self else {
                ready.signal()
                return
            }
            self.run(ready: ready)
        }
        thread.name = "pigeon.render"
        thread.qualityOfService = .userInteractive
        self.thread = thread
        thread.start()
        // Posts go to the thread's run loop, so it must exist before `start` returns.
        ready.wait()
    }

    /// Stops the thread; the caller drops the renderer afterwards.
    func stop() {
        mailbox.close()
        perform { [self] in
            if #available(macOS 14.0, *), let link = link as? CAMetalDisplayLink {
                link.invalidate()
            }
            link = nil
            linkDelegate = nil
            CFRunLoopStop(CFRunLoopGetCurrent())
        }
        thread = nil
    }

    func post(_ frame: GridFrame) {
        mailbox.post(frame)
        perform { [self] in
            if #available(macOS 14.0, *), let link = link as? CAMetalDisplayLink {
                link.isPaused = false
            } else {
                drawPending(drawable: nil)
            }
        }
    }

    /// Runs `block` on the render thread.
    private func perform(_ block: @escaping () -> Void) {
        guard let runLoop else { return }
        CFRunLoopPerformBlock(runLoop, CFRunLoopMode.defaultMode.rawValue, block)
        CFRunLoopWakeUp(runLoop)
    }

    private func run(ready: DispatchSemaphore) {
        runLoop = CFRunLoopGetCurrent()
        if #available(macOS 14.0, *) {
            let delegate = DisplayLinkDelegate { [weak self] update in
                self?.drawPending(drawable: update.drawable)
            }
            let link = CAMetalDisplayLink(metalLayer: layer)
            link.delegate = delegate
            // ProMotion panels run at up to 120Hz; the link drops to the display's own rate.
            link.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
            link.preferredFrameLatency = 1
            link.isPaused = true
            link.add(to: .current, forMode: .default)
            self.link = link
            linkDelegate = delegate
        }
        // A run loop with no sources returns at once; the port keeps it alive between posts.
        RunLoop.current.add(Port(), forMode: .default)
        ready.signal()
        while !mailbox.isClosed {
            _ = autoreleasepool {
                RunLoop.current.run(mode: .default, before: .distantFuture)
            }
        }
        // Wait for the GPU to finish with every slot before the buffers go away.
        for _ in 0..<Self.inFlight { slots.wait() }
        for _ in 0..<Self.inFlight { slots.signal() }
    }

    /// Draws the newest posted frame, or pauses the link when nothing is waiting.
    private func drawPending(drawable: CAMetalDrawable?) {
        guard let frame = mailbox.poll() else {
            if #available(macOS 14.0, *), let link = link as? CAMetalDisplayLink, !link.isPaused {
                link.isPaused = true
                if FrameStats.enabled { stats.flush() }
            }
            return
        }
        autoreleasepool { render(frame, drawable: drawable) }
    }

    private func render(_ frame: GridFrame, drawable linked: CAMetalDrawable?) {
        guard frame.width > 0, frame.height > 0 else { return }
        let size = CGSize(width: frame.width, height: frame.height)
        if layer.drawableSize != size {
            layer.drawableSize = size
            // The link handed out a drawable of the old size; drawing into it would stretch
            // this frame. Keep the frame for the next refresh, which brings a matching one.
            if linked != nil {
                mailbox.restore(frame)
                return
            }
        }
        if glyphs == nil { glyphs = GlyphCache(device: device) }
        guard let glyphs else { return }

        let start = Self.debug || FrameStats.enabled ? DispatchTime.now().uptimeNanoseconds : 0
        instances.update(from: frame, atlasGeneration: glyphs.generation) { key in
            glyphs.slot(for: key, metrics: frame.metrics)
        }
        // A frame that filled the atlas would draw with missing glyphs: start the atlas over
        // and rebuild every row once against the empty one.
        if glyphs.resetIfOverflowed() {
            instances.update(from: frame, atlasGeneration: glyphs.generation) { key in
                glyphs.slot(for: key, metrics: frame.metrics)
            }
        }

        slots.wait()
        guard let drawable = linked ?? layer.nextDrawable(),
              let commands = queue.makeCommandBuffer()
        else {
            slots.signal()
            // No drawable (window hidden, timeout): redraw everything when one comes back.
            instances.invalidate()
            return
        }
        let (backgrounds, decorations) = instances.rects(overlays: GridInstances.overlays(for: frame))
        let glyphList = instances.glyphs()

        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = drawable.texture
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].storeAction = .store
        let clear = CellPalette.components(frame.palette.background)
        pass.colorAttachments[0].clearColor = MTLClearColor(
            red: Double(clear.x), green: Double(clear.y), blue: Double(clear.z), alpha: 1)

        guard let encoder = commands.makeRenderCommandEncoder(descriptor: pass) else {
            slots.signal()
            return
        }
        var uniforms = SIMD2<Float>(Float(frame.width), Float(frame.height))
        encoder.setVertexBytes(&uniforms, length: MemoryLayout<SIMD2<Float>>.stride, index: 1)

        encoder.setRenderPipelineState(rectPipeline)
        draw(backgrounds, buffer: 0, encoder: encoder)
        encoder.setRenderPipelineState(glyphPipeline)
        encoder.setFragmentTexture(glyphs.mask, index: 0)
        encoder.setFragmentTexture(glyphs.color, index: 1)
        draw(glyphList, buffer: 1, encoder: encoder)
        encoder.setRenderPipelineState(rectPipeline)
        draw(decorations, buffer: 2, encoder: encoder)
        encoder.endEncoding()

        let semaphore = slots
        let measure = FrameStats.enabled
        commands.addCompletedHandler { [weak self] buffer in
            semaphore.signal()
            guard measure, buffer.gpuEndTime > buffer.gpuStartTime else { return }
            let micros = (buffer.gpuEndTime - buffer.gpuStartTime) * 1_000_000
            self?.perform { self?.stats.record(gpuMicros: micros) }
        }
        commands.present(drawable)
        commands.commit()
        slot = (slot + 1) % Self.inFlight

        if FrameStats.enabled {
            stats.record(cpuMicros: Double(DispatchTime.now().uptimeNanoseconds - start) / 1000)
            stats.reportIfDue()
        }
        if Self.debug {
            let micros = (DispatchTime.now().uptimeNanoseconds - start) / 1000
            FileHandle.standardError.write(Data(
                "pigeon: gpu frame rows=\(instances.lastRebuilt)/\(frame.rows) glyphs=\(glyphList.count) cpu=\(micros)us\n".utf8))
        }
    }

    /// Copies `items` into this frame's slot of buffer `index` (growing it when needed) and
    /// issues one instanced draw of six vertices per item.
    private func draw<T>(_ items: [T], buffer index: Int, encoder: MTLRenderCommandEncoder) {
        guard !items.isEmpty else { return }
        let length = items.count * MemoryLayout<T>.stride
        let buffer: MTLBuffer
        if let existing = buffers[slot][index], existing.length >= length {
            buffer = existing
        } else {
            // Grow geometrically so a busy screen does not reallocate every frame.
            guard let grown = device.makeBuffer(
                length: max(length * 3 / 2, 4096), options: .storageModeShared)
            else { return }
            buffers[slot][index] = grown
            buffer = grown
        }
        items.withUnsafeBytes { bytes in
            guard let base = bytes.baseAddress else { return }
            buffer.contents().copyMemory(from: base, byteCount: length)
        }
        encoder.setVertexBuffer(buffer, offset: 0, index: 0)
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6, instanceCount: items.count)
    }
}

/// Forwards `CAMetalDisplayLink` callbacks to the renderer.
@available(macOS 14.0, *)
private final class DisplayLinkDelegate: NSObject, CAMetalDisplayLinkDelegate {
    let onUpdate: (CAMetalDisplayLink.Update) -> Void

    init(_ onUpdate: @escaping (CAMetalDisplayLink.Update) -> Void) {
        self.onUpdate = onUpdate
    }

    func metalDisplayLink(_ link: CAMetalDisplayLink, needsUpdate update: CAMetalDisplayLink.Update) {
        onUpdate(update)
    }
}
