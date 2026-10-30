import Foundation

/// Frame-time samples for one pane, reported as p50/p99 once a second while frames are
/// being drawn (issue #1 "衡量标准"). Enabled by `PIGEON_FRAME_STATS=1`; owned by whichever
/// thread draws the pane, so it needs no lock.
struct FrameStats {
    static let enabled = ProcessInfo.processInfo.environment["PIGEON_FRAME_STATS"] != nil

    let renderer: String
    private(set) var cpu: [Double] = []
    private(set) var gpu: [Double] = []
    private var windowStart: UInt64 = 0

    init(renderer: String) {
        self.renderer = renderer
    }

    /// Microseconds spent building and encoding one frame.
    mutating func record(cpuMicros: Double) {
        cpu.append(cpuMicros)
    }

    /// Microseconds the GPU spent on one frame, from the command buffer's timestamps.
    mutating func record(gpuMicros: Double) {
        gpu.append(gpuMicros)
    }

    /// The value at percentile `p` (0…100), nearest-rank; nil without samples.
    static func percentile(_ samples: [Double], _ p: Double) -> Double? {
        guard !samples.isEmpty else { return nil }
        let sorted = samples.sorted()
        let rank = Int((p / 100 * Double(sorted.count)).rounded(.up))
        return sorted[min(max(rank - 1, 0), sorted.count - 1)]
    }

    /// One log line for the samples so far, e.g.
    /// `pigeon: frames renderer=metal n=58 cpu_p50=410us cpu_p99=1900us gpu_p50=120us gpu_p99=300us`.
    func summary() -> String? {
        guard let p50 = Self.percentile(cpu, 50), let p99 = Self.percentile(cpu, 99) else { return nil }
        var line = "pigeon: frames renderer=\(renderer) n=\(cpu.count)"
            + " cpu_p50=\(Int(p50))us cpu_p99=\(Int(p99))us"
        if let g50 = Self.percentile(gpu, 50), let g99 = Self.percentile(gpu, 99) {
            line += " gpu_p50=\(Int(g50))us gpu_p99=\(Int(g99))us"
        }
        return line
    }

    /// Writes and clears the summary when a second has passed since the window opened.
    mutating func reportIfDue(now: UInt64 = DispatchTime.now().uptimeNanoseconds) {
        if windowStart == 0 { windowStart = now }
        guard now - windowStart >= 1_000_000_000 else { return }
        flush()
        windowStart = now
    }

    /// Writes whatever was collected and starts over. Called when drawing goes idle, so a
    /// burst shorter than a second is still reported.
    mutating func flush() {
        if let line = summary() {
            FileHandle.standardError.write(Data((line + "\n").utf8))
        }
        cpu = []
        gpu = []
        windowStart = 0
    }
}
