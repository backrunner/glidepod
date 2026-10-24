import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

// A continuous radial fill: a luminous center supports the glass packages,
// with a soft falloff toward the emerald perimeter. Package layers stay vector.
struct Stop: Decodable { let position: Double; let color: String }
struct Configuration: Decodable {
    let size: Int
    let center: [Double]
    let radius: Double
    let stops: [Stop]
}
let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    fatalError("Usage: generate-icon-background <configuration.json> <output.png>")
}
let config = try JSONDecoder().decode(Configuration.self, from: Data(contentsOf: URL(fileURLWithPath: arguments[1])))
precondition(config.size > 0 && config.center.count == 2 && config.stops.count >= 2)
// An untransformed PNG layer uses its intrinsic size in the 1024-unit icon
// canvas. A larger bitmap is center-cropped rather than automatically fitted.
precondition(config.size == 1024, "Background must match Icon Composer's canvas")
precondition(config.stops.first!.position == 0 && config.stops.last!.position == 1)
precondition(config.radius > 0)
precondition(zip(config.stops, config.stops.dropFirst()).allSatisfy { $0.position < $1.position })
func channels(_ color: String) -> [Double] {
    let rgb = UInt32(color.dropFirst(), radix: 16)!
    return [Double((rgb >> 16) & 255), Double((rgb >> 8) & 255), Double(rgb & 255)]
}
let colors = config.stops.map { channels($0.color) }
let size = config.size
var pixels = [UInt8](repeating: 255, count: size * size * 4)
for y in 0..<size {
    for x in 0..<size {
        let dx = (Double(x) + 0.5) / Double(size) - config.center[0]
        let dy = (Double(y) + 0.5) / Double(size) - config.center[1]
        let t = min(1, hypot(dx, dy) / config.radius)
        let end = config.stops.indices.dropFirst().first { t <= config.stops[$0].position }!
        let start = end - 1
        let fraction = (t - config.stops[start].position) / (config.stops[end].position - config.stops[start].position)
        let weight = fraction * fraction * (3 - 2 * fraction)
        for channel in 0..<3 {
            let value = colors[start][channel] + (colors[end][channel] - colors[start][channel]) * weight
            pixels[(y * size + x) * 4 + channel] = UInt8(value.rounded())
        }
    }
}
let data = Data(pixels)
let image = CGImage(width: size, height: size, bitsPerComponent: 8, bitsPerPixel: 32,
    bytesPerRow: size * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
    bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
    provider: CGDataProvider(data: data as CFData)!, decode: nil, shouldInterpolate: true,
    intent: .defaultIntent)!
let url = URL(fileURLWithPath: arguments[2])
let destination = CGImageDestinationCreateWithURL(url as CFURL, UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(destination, image, nil)
guard CGImageDestinationFinalize(destination) else { fatalError("Cannot write background") }
