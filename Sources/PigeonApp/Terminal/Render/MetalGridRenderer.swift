import Foundation
import Metal
import QuartzCore

/// Draws one pane's grid on the GPU from its own thread (issue #1).
///
/// The main thread only posts `GridFrame` values into a `FrameMailbox`; this thread takes
/// the newest one, rebuilds the instance data of the dirty rows, and encodes one instanced
/// draw per pass (backgrounds, glyphs, decorations + cursor). Nothing runs while no frame is
/// posted, so an idle pane costs no CPU or GPU time.
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

    // Render-thread state.
    private var glyphs: GlyphCache?
    private var instances = GridInstances()
    private var buffers: [[MTLBuffer?]] = Array(
        repeating: [nil, nil, nil], count: MetalGridRenderer.inFlight)
    private var slot = 0

    private static let debug = ProcessInfo.processInfo.environment["PIGEON_DEBUG"] != nil

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
        let thread = Thread { [weak self] in self?.run() }
        thread.name = "pigeon.render"
        thread.qualityOfService = .userInteractive
        self.thread = thread
        thread.start()
    }

    /// Stops the thread; the caller drops the renderer afterwards.
    func stop() {
        mailbox.close()
        thread = nil
    }

    func post(_ frame: GridFrame) {
        mailbox.post(frame)
    }

    private func run() {
        while let frame = mailbox.take() {
            autoreleasepool { render(frame) }
        }
        // Wait for the GPU to finish with every slot before the buffers go away.
        for _ in 0..<Self.inFlight { slots.wait() }
        for _ in 0..<Self.inFlight { slots.signal() }
    }

    private func render(_ frame: GridFrame) {
        guard frame.width > 0, frame.height > 0 else { return }
        if glyphs == nil { glyphs = GlyphCache(device: device) }
        guard let glyphs else { return }

        let start = Self.debug ? DispatchTime.now().uptimeNanoseconds : 0
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

        let size = CGSize(width: frame.width, height: frame.height)
        if layer.drawableSize != size { layer.drawableSize = size }

        slots.wait()
        guard let drawable = layer.nextDrawable(),
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
        commands.addCompletedHandler { _ in semaphore.signal() }
        commands.present(drawable)
        commands.commit()
        slot = (slot + 1) % Self.inFlight

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
