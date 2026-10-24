import AppKit
import ImageIO
import UniformTypeIdentifiers

// Export a complete ICNS fallback from the compiled catalog, preserving the
// system's icon inset and lighting instead of enlarging actool's partial ICNS.
// Initialize AppKit so its NSBundle image-resource category is registered in a
// standalone command-line process too.
_ = NSApplication.shared
let arguments = CommandLine.arguments
guard arguments.count == 3,
      let bundle = Bundle(path: arguments[1]),
      let icon = bundle.image(forResource: NSImage.Name("GlidePod")) else {
    fatalError("Usage: ExportAppIcon <catalog.bundle> <output.iconset>")
}
let output = URL(fileURLWithPath: arguments[2], isDirectory: true)
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
for points in [16, 32, 128, 256, 512] {
    for scale in [1, 2] {
        let pixels = points * scale
        var bounds = CGRect(x: 0, y: 0, width: pixels, height: pixels)
        guard let source = icon.cgImage(forProposedRect: &bounds, context: nil, hints: nil),
              let context = CGContext(
                data: nil, width: pixels, height: pixels, bitsPerComponent: 8,
                bytesPerRow: 0, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
              ) else { fatalError("Cannot render \(pixels)px icon") }
        precondition(source.width >= pixels && source.height >= pixels,
                     "Catalog must supply real pixels, not an enlarged fallback")
        context.interpolationQuality = .high
        context.draw(source, in: bounds)
        let suffix = scale == 2 ? "@2x" : ""
        let url = output.appendingPathComponent("icon_\(points)x\(points)\(suffix).png")
        guard let image = context.makeImage(),
              let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)
        else { fatalError("Cannot write \(url.path)") }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination) else { fatalError("Cannot finish \(url.path)") }
    }
}
