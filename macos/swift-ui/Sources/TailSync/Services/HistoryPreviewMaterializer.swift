import AppKit
import Foundation
import PDFKit

/// Converts decrypted bytes into a renderer-specific material without writing
/// plaintext unless Quick Look requires a file URL (currently DOCX only).
extension HistoryPreviewStore {
    func materialize(_ preview: HistoryPreviewData) throws -> HistoryPreviewMaterial {
        guard preview.sizeBytes >= 0,
              preview.sizeBytes <= HistoryPreviewData.maxBytes,
              Int64(preview.data.count) <= HistoryPreviewData.maxBytes else {
            throw HistoryPreviewStoreError.tooLarge
        }

        let fileExtension = HistoryPreviewFileTypes.fileExtension(for: preview.name)
        if preview.kind == "text" || HistoryPreviewFileTypes.textExtensions.contains(fileExtension) {
            guard let text = String(data: preview.data, encoding: .utf8) else {
                throw HistoryPreviewStoreError.invalidText
            }
            return .text(text)
        }

        if preview.kind == "image" {
            let png = try pngData(fromPackedRGBA: preview.data)
            guard NSImage(data: png) != nil else {
                throw HistoryPreviewStoreError.invalidImage
            }
            return .image(png)
        }
        if HistoryPreviewFileTypes.imageExtensions.contains(fileExtension) {
            guard NSImage(data: preview.data) != nil else {
                throw HistoryPreviewStoreError.invalidImage
            }
            return .image(preview.data)
        }
        if fileExtension == "pdf" {
            guard PDFDocument(data: preview.data) != nil else {
                throw HistoryPreviewStoreError.invalidDocument
            }
            return .pdf(preview.data)
        }
        if fileExtension == "docx" {
            guard preview.data.count >= 4,
                  preview.data.prefix(4).elementsEqual([0x50, 0x4B, 0x03, 0x04]) else {
                throw HistoryPreviewStoreError.invalidDocument
            }
            let fileName = HistoryPreviewStore.sanitizedFileName(preview.name, fallback: "preview.docx")
            return .quickLook(try write(preview.data, named: fileName))
        }
        return .unsupported
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
