import AppKit
import Foundation
import ImageIO
import PDFKit

enum HistoryPreviewImageLimits {
    static let maximumDecodedPixels = 64 * 1024 * 1024

    static func accepts(frameDimensions: [(width: Int, height: Int)]) -> Bool {
        guard !frameDimensions.isEmpty else { return false }
        var totalPixels = 0
        for frame in frameDimensions {
            guard frame.width > 0, frame.height > 0 else { return false }
            let framePixels = frame.width.multipliedReportingOverflow(by: frame.height)
            guard !framePixels.overflow else { return false }
            let accumulated = totalPixels.addingReportingOverflow(framePixels.partialValue)
            guard !accumulated.overflow,
                  accumulated.partialValue <= maximumDecodedPixels else { return false }
            totalPixels = accumulated.partialValue
        }
        return true
    }
}

enum HistoryPreviewDocumentSignatures {
    static func isLikelyDOCX(_ data: Data) -> Bool {
        isLikelyOpenXML(data, requiredEntry: "word/document.xml")
    }

    static func isLikelyPPTX(_ data: Data) -> Bool {
        isLikelyOpenXML(data, requiredEntry: "ppt/presentation.xml")
    }

    static func isLikelyLegacyPPT(_ data: Data) -> Bool {
        data.starts(with: [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
    }

    private static func isLikelyOpenXML(_ data: Data, requiredEntry: String) -> Bool {
        guard data.starts(with: [0x50, 0x4B, 0x03, 0x04]) else { return false }
        let requiredEntries = ["[Content_Types].xml", requiredEntry]
        guard requiredEntries.allSatisfy({ data.range(of: Data($0.utf8)) != nil }) else {
            return false
        }
        return data.range(of: Data([0x50, 0x4B, 0x05, 0x06])) != nil
    }
}

/// Converts decrypted bytes into a renderer-specific material without writing
/// plaintext unless Quick Look requires a file URL (Office documents only).
extension HistoryPreviewStore {
    func materialize(_ preview: HistoryPreviewData) throws -> HistoryPreviewMaterial {
        guard preview.sizeBytes >= 0,
              preview.sizeBytes <= HistoryPreviewData.maxBytes,
              Int64(preview.data.count) <= HistoryPreviewData.maxBytes else {
            throw HistoryPreviewStoreError.tooLarge
        }

        let fileExtension = HistoryPreviewFileTypes.fileExtension(for: preview.name)
        if fileExtension == "svg" {
            guard let source = String(data: preview.data, encoding: .utf8) else {
                throw HistoryPreviewStoreError.invalidText
            }
            // The browser-engine snapshot renderer takes over in the view
            // model; this base material keeps the escaped source in memory
            // and is what the preview shows if that render fails.
            return .text(source)
        }
        if preview.kind == "text" || HistoryPreviewFileTypes.textExtensions.contains(fileExtension) {
            guard let text = String(data: preview.data, encoding: .utf8) else {
                throw HistoryPreviewStoreError.invalidText
            }
            return .text(text)
        }

        if preview.kind == "image" {
            let png = try pngData(fromPackedRGBA: preview.data)
            let image = try validatedImage(from: png)
            return .image(HistoryPreviewImageMaterial(data: png, image: image))
        }
        if HistoryPreviewFileTypes.imageExtensions.contains(fileExtension) {
            let image = try validatedImage(from: preview.data)
            return .image(HistoryPreviewImageMaterial(data: preview.data, image: image))
        }
        if fileExtension == "pdf" {
            guard let document = PDFDocument(data: preview.data),
                  !document.isLocked,
                  document.pageCount > 0 else {
                throw HistoryPreviewStoreError.invalidDocument
            }
            return .pdf(HistoryPreviewPDFMaterial(data: preview.data, document: document))
        }
        if fileExtension == "docx" {
            guard HistoryPreviewDocumentSignatures.isLikelyDOCX(preview.data) else {
                throw HistoryPreviewStoreError.invalidDocument
            }
            let fileName = HistoryPreviewStore.sanitizedFileName(preview.name, fallback: "preview.docx")
            return .quickLook(try write(preview.data, named: fileName))
        }
        if fileExtension == "pptx" {
            guard HistoryPreviewDocumentSignatures.isLikelyPPTX(preview.data) else {
                throw HistoryPreviewStoreError.invalidDocument
            }
            let fileName = HistoryPreviewStore.sanitizedFileName(
                preview.name,
                fallback: "preview.pptx"
            )
            return .quickLook(try write(preview.data, named: fileName))
        }
        if fileExtension == "ppt" {
            guard HistoryPreviewDocumentSignatures.isLikelyLegacyPPT(preview.data) else {
                throw HistoryPreviewStoreError.invalidDocument
            }
            let fileName = HistoryPreviewStore.sanitizedFileName(
                preview.name,
                fallback: "preview.ppt"
            )
            return .quickLook(try write(preview.data, named: fileName))
        }
        return .unsupported
    }

    private func validatedImage(from data: Data) throws -> NSImage {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else {
            throw HistoryPreviewStoreError.invalidImage
        }
        let dimensions = (0..<CGImageSourceGetCount(source)).compactMap { index
            -> (width: Int, height: Int)? in
            guard let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil)
                    as? [CFString: Any],
                  let width = properties[kCGImagePropertyPixelWidth] as? Int,
                  let height = properties[kCGImagePropertyPixelHeight] as? Int else { return nil }
            return (width, height)
        }
        guard HistoryPreviewImageLimits.accepts(frameDimensions: dimensions),
              let image = NSImage(data: data) else {
            throw HistoryPreviewStoreError.invalidImage
        }
        return image
    }

    private func pngData(fromPackedRGBA packed: Data) throws -> Data {
        guard packed.count >= 8 else { throw HistoryPreviewStoreError.invalidImage }
        let width = UInt32(packed[0])
            | (UInt32(packed[1]) << 8)
            | (UInt32(packed[2]) << 16)
            | (UInt32(packed[3]) << 24)
        let height = UInt32(packed[4])
            | (UInt32(packed[5]) << 8)
            | (UInt32(packed[6]) << 16)
            | (UInt32(packed[7]) << 24)
        guard width > 0, height > 0 else {
            throw HistoryPreviewStoreError.invalidImage
        }
        let pixelCountResult = Int(width).multipliedReportingOverflow(by: Int(height))
        guard !pixelCountResult.overflow,
              pixelCountResult.partialValue <= 64 * 1024 * 1024 else {
            throw HistoryPreviewStoreError.invalidImage
        }
        let byteCountResult = pixelCountResult.partialValue.multipliedReportingOverflow(by: 4)
        guard !byteCountResult.overflow, byteCountResult.partialValue == packed.count - 8 else {
            throw HistoryPreviewStoreError.invalidImage
        }

        let rgba = packed.dropFirst(8)
        guard let provider = CGDataProvider(data: Data(rgba) as CFData),
              let colorSpace = CGColorSpace(name: CGColorSpace.sRGB) else {
            throw HistoryPreviewStoreError.invalidImage
        }
        let bitmapInfo = CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)
        guard let image = CGImage(
            width: Int(width),
            height: Int(height),
            bitsPerComponent: 8,
            bitsPerPixel: 32,
            bytesPerRow: Int(width) * 4,
            space: colorSpace,
            bitmapInfo: bitmapInfo,
            provider: provider,
            decode: nil,
            shouldInterpolate: false,
            intent: .defaultIntent
        ) else {
            throw HistoryPreviewStoreError.invalidImage
        }
        let representation = NSBitmapImageRep(cgImage: image)
        guard let png = representation.representation(using: .png, properties: [:]) else {
            throw HistoryPreviewStoreError.invalidImage
        }
        guard Int64(png.count) <= HistoryPreviewData.maxBytes else {
            throw HistoryPreviewStoreError.tooLarge
        }
        return png
    }
}
