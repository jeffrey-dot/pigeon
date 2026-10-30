/// Which grid renderer a pane uses. The Metal renderer stays behind
/// `PIGEON_RENDERER=metal` until it matches the CoreText path (issue #1); `coretext`
/// (the default) keeps the CPU renderer.
enum RendererChoice: Equatable {
    case coreText
    case metal

    static func fromEnvironment(_ value: String?) -> RendererChoice {
        value?.lowercased() == "metal" ? .metal : .coreText
    }
}
