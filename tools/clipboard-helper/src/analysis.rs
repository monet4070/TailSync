use std::io::Read;
use tailsync_protocol::content_analysis::{scan_rtf, SavedTextAnalysis};

pub const SAVED_ANALYSIS_INPUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisCommand {
    Rtf,
    Rtfd,
}

pub fn command(name: &str) -> Option<AnalysisCommand> {
    match name {
        "--analyze-rtf" => Some(AnalysisCommand::Rtf),
        "--analyze-rtfd" => Some(AnalysisCommand::Rtfd),
        _ => None,
    }
}

pub fn read_bounded_stdin() -> Result<Vec<u8>, String> {
    let mut input = vec![];
    std::io::stdin()
        .take(SAVED_ANALYSIS_INPUT_LIMIT as u64 + 1)
        .read_to_end(&mut input)
        .map_err(|error| error.to_string())?;
    if input.len() > SAVED_ANALYSIS_INPUT_LIMIT {
        return Err("saved analysis input exceeds limit".into());
    }
    Ok(input)
}

pub fn analyze(command: AnalysisCommand, bytes: &[u8]) -> Result<SavedTextAnalysis, String> {
    match command {
        AnalysisCommand::Rtf => Ok(scan_rtf(bytes)),
        AnalysisCommand::Rtfd => Err("RTFD attachment analysis requires the AppKit helper".into()),
    }
}

pub fn encode_or_exit(analysis: &SavedTextAnalysis) -> Vec<u8> {
    match analysis.encode() {
        Ok(frame) => frame,
        Err(error) => {
            eprintln!("saved analysis frame rejected: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_analysis_arguments_select_the_read_only_mode() {
        assert_eq!(command("--analyze-rtf"), Some(AnalysisCommand::Rtf));
        assert_eq!(command("--analyze-rtfd"), Some(AnalysisCommand::Rtfd));
        assert_eq!(command("--write-snapshot"), None);
        assert_eq!(command("--read-snapshot"), None);
    }

    #[test]
    fn rtfd_is_explicitly_unsupported_rather_than_guessed() {
        assert!(analyze(AnalysisCommand::Rtfd, b"anything").is_err());
    }

    #[test]
    fn rtf_scan_round_trips_owned_attachment_bytes() {
        let png = [0x89u8, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4];
        let hex: String = png.iter().map(|byte| format!("{byte:02x}")).collect();
        let rtf = format!("{{\\rtf1\\ansi \\u20013?\\pict\\pngblip {hex}}}");
        let analysis = analyze(AnalysisCommand::Rtf, rtf.as_bytes()).unwrap();
        assert_eq!(analysis.attachments.len(), 1);
        assert_eq!(
            analysis.attachments[0].state,
            tailsync_protocol::content_analysis::AttachmentState::Available
        );
        let frame = analysis.encode().unwrap();
        assert_eq!(SavedTextAnalysis::decode(&frame).unwrap(), analysis);
    }
}
