//! Font validation shared by language-pack loading, authoring and UI insertion.

/// Failure before a language-pack font can enter a renderer.
#[derive(Debug)]
pub enum LocaleFontError {
    Read(std::io::Error),
    Parse,
}

impl std::fmt::Display for LocaleFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(error) => write!(f, "cannot read font file: {error}"),
            Self::Parse => write!(f, "file is not a valid font"),
        }
    }
}

impl std::error::Error for LocaleFontError {}

/// Read and validate the exact bytes the UI will insert. Uses epaint's parser
/// without depending on egui or a graphics context. Does not resolve families,
/// reorder fallback fonts, or shape RTL text.
pub fn read_validated(path: &std::path::Path) -> Result<Vec<u8>, LocaleFontError> {
    let bytes = std::fs::read(path).map_err(LocaleFontError::Read)?;
    ab_glyph::FontRef::try_from_slice(&bytes).map_err(|_| LocaleFontError::Parse)?;
    Ok(bytes)
}
