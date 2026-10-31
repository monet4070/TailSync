# TailSync WPF third-party notices

The WPF frontend uses the following open-source components. Full supplied
license texts are in `Licenses/` alongside the application. The Rust backend
retains TailSync's existing Cargo dependencies and license requirements.

| Component | Version | License | Source |
| --- | --- | --- | --- |
| AvalonEdit | 6.3.1.120 | MIT | https://github.com/icsharpcode/AvalonEdit |
| Markdig | 1.5.1 | BSD-2-Clause | https://github.com/xoofx/markdig |
| SharpVectors.Wpf | 1.8.6 | BSD-3-Clause | https://github.com/ElinamLLC/SharpVectors |
| PDFtoImage | 5.4.0 | MIT | https://github.com/sungaila/PDFtoImage |
| PdfPig | 0.1.16 | Apache-2.0 | https://github.com/UglyToad/PdfPig |
| SkiaSharp / native Skia | 4.150.1 | MIT / BSD-3-Clause and bundled notices | https://github.com/mono/SkiaSharp |
| bblanchon.PDFium.Win32 | 152.0.7961 | Apache-2.0 package metadata; upstream PDFium BSD-style and third-party notices | https://github.com/bblanchon/pdfium-binaries |
| .NET / WPF runtime | 10.0 | MIT and runtime third-party notices | https://github.com/dotnet/wpf |

Windows-installed fonts are selected through Theme V2's ordered fallback
families. TailSync does not redistribute macOS fonts or proprietary fonts.

UI symbols use Unicode glyphs from the Windows-installed Segoe Fluent Icons
font, with per-glyph fallback to Segoe MDL2 Assets. Neither font is bundled.
